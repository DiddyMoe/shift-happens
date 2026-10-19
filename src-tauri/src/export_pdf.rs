//! Letter landscape, locally embedded Noto Sans. V1 supports NFC Latin text
//! in the declared ranges and a small set of Common punctuation/symbols.
//! Other scripts, residual combining marks, bidi controls and variation selectors
//! fail preflight; no shaping/bidi support is claimed for them.
use crate::{
    error::{AppError, AppResult, ErrorCode},
    export_snapshot::{COMPACT_HEADERS, ExportCell, ExportSnapshot, HEADERS},
    export_xlsx::{generation_error, validation_error},
    safe_destination::{self, OwnedTemporary},
};
use pdf_writer::{
    Content, Finish, Name, Pdf, Rect, Ref, Str,
    types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap},
};
use skrifa::{
    FontRef, MetadataProvider,
    instance::{LocationRef, Size},
    metrics::Metrics,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
};
use unicode_normalization::UnicodeNormalization;
use unicode_script::{Script, UnicodeScript};

const FONT: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
const WIDTHS: [f32; 10] = [60., 54., 62., 42., 63., 115., 96., 101., 40., 87.];
const MARGIN: f32 = 36.;
const BODY_BOTTOM: f32 = 60.;
const BODY_TOP: f32 = 456.;
const LINE: f32 = 14.;
const SIZE: f32 = 10.;
// Source limits bound text bytes, not layout: accepted notes may hold thousands
// of one-character lines, each a retained run. Every page holds a bounded number
// of runs (a detail page at most 29 body lines plus metadata, heading and footer;
// a table page at most 11 rows of ten two-line cells), so admitting pages bounds
// retained runs, PDF content and readback work. By estimate from 2,008 ordinary
// rows producing 1,051 pages, 10,000 rows need about 5,200 pages, and a full
// 4,000-character prose note on every row stays under 20,000.
const PAGE_LIMIT: usize = 25_000;
pub const SUPPORT: &str = "NFC Latin (U+0020–007E, U+00A0–024F, U+1E00–1EFF), selected punctuation and symbols, with verified Noto Sans glyphs. Other scripts, remaining combining marks and bidirectional text are unsupported.";

fn supported(c: char) -> bool {
    c.is_ascii() && (c == '\n' || c == '\t' || !c.is_control())
        || (c.script() == Script::Latin && matches!(u32::from(c),0x00a0..=0x024f|0x1e00..=0x1eff))
        || matches!(
            c,
            '\u{00a0}'
                | '¡'
                | '¢'
                | '£'
                | '¤'
                | '¥'
                | '¦'
                | '§'
                | '¨'
                | '©'
                | 'ª'
                | '«'
                | '¬'
                | '®'
                | '¯'
                | '°'
                | '±'
                | '²'
                | '³'
                | '´'
                | 'µ'
                | '¶'
                | '·'
                | '¸'
                | '¹'
                | 'º'
                | '»'
                | '¼'
                | '½'
                | '¾'
                | '¿'
                | '×'
                | '÷'
                | '‐'
                | '‑'
                | '‒'
                | '–'
                | '—'
                | '‘'
                | '’'
                | '‚'
                | '“'
                | '”'
                | '„'
                | '†'
                | '‡'
                | '•'
                | '…'
                | '‰'
                | '‹'
                | '›'
                | '€'
                | '™'
        )
}
pub struct PdfFont {
    metrics: Metrics,
    // Distinct Unicode scalars have distinct CIDs even if the font shares a
    // glyph (e.g. spaces). Thus ToUnicode never silently loses text identity.
    chars: BTreeMap<char, PdfGlyph>,
}
struct PdfGlyph {
    cid: u16,
    glyph_id: u16,
    advance: f32,
}
impl PdfFont {
    fn width(&self, text: &str, size: f32) -> f32 {
        text.chars()
            .map(|c| {
                self.chars.get(&c).map_or(0., |glyph| glyph.advance) * size
                    / f32::from(self.metrics.units_per_em)
            })
            .sum()
    }
    fn bytes(&self, text: &str) -> AppResult<Vec<u8>> {
        let mut bytes = Vec::with_capacity(text.len() * 2);
        for c in text.chars() {
            let glyph = self.chars.get(&c).ok_or_else(generation_error)?;
            bytes.extend_from_slice(&glyph.cid.to_be_bytes());
        }
        Ok(bytes)
    }
}
pub fn preflight(snapshot: &ExportSnapshot) -> AppResult<PdfFont> {
    // Parse only the bundled, static font. Cache its nominal glyphs and unscaled
    // metrics during preflight; layout and PDF embedding then use exactly the
    // same verified advances without loading system fonts or shaping machinery.
    let font = FontRef::new(FONT).map_err(|_| generation_error())?;
    let charmap = font.charmap();
    let metrics = font.metrics(Size::unscaled(), LocationRef::default());
    let glyph_metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    if !charmap.has_map() || metrics.units_per_em == 0 || metrics.bounds.is_none() {
        return Err(generation_error());
    }
    let mut chars = BTreeSet::new();
    let mut unsupported = BTreeSet::new();
    let mut script_counts = BTreeMap::<&'static str, usize>::new();
    let mut unsupported_count = 0usize;
    let mut run_count = 0usize;
    let mut inspect = |text: &str| {
        let mut bad = false;
        for c in text.nfc() {
            if !supported(c)
                || (!matches!(c, '\n' | '\t')
                    && charmap.map(c).is_none_or(|glyph| {
                        glyph.to_u32() == 0 || glyph_metrics.advance_width(glyph).is_none()
                    }))
            {
                unsupported.insert(c);
                unsupported_count += 1;
                *script_counts.entry(c.script().short_name()).or_default() += 1;
                bad = true;
            } else if !matches!(c, '\n' | '\t') {
                chars.insert(c);
            }
        }
        if bad {
            run_count += 1;
        }
    };
    // Include every possible generated label/footer and visible truncation mark.
    inspect(
        "0123456789 Page of Assignment details Schedule metadata Notes Detail No assignments - / : | …",
    );
    for header in HEADERS.into_iter().chain(COMPACT_HEADERS) {
        inspect(header);
    }
    for value in snapshot.metadata() {
        inspect(&value);
    }
    inspect(&snapshot.schedule_notes);
    for row in &snapshot.rows {
        for cell in &row.cells {
            if let ExportCell::Text(v) = cell {
                inspect(v);
            }
        }
        for value in &row.compact {
            inspect(value);
        }
        for field in &row.details {
            inspect(field.label);
            inspect(&field.value);
        }
    }
    if unsupported_count > 0 {
        let mut error = AppError::new(
            ErrorCode::PdfTextNotRenderable,
            "Some text cannot be faithfully rendered by the bundled PDF font/script support. No PDF was created. Use XLSX when available.",
        );
        error.context.diagnostics = Some(serde_json::json!({
            "support":SUPPORT,"unsupported_code_point_count":unsupported_count,
            "distinct_unsupported_code_point_count":unsupported.len(),"unsupported_run_count":run_count,
            "script_counts":script_counts,"xlsx_alternative":snapshot.xlsx_available
        }));
        return Err(error);
    }
    let chars = chars
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let glyph = charmap.map(c).ok_or_else(generation_error)?;
            Ok((
                c,
                PdfGlyph {
                    cid: u16::try_from(i + 1).map_err(|_| generation_error())?,
                    glyph_id: u16::try_from(glyph.to_u32()).map_err(|_| generation_error())?,
                    advance: glyph_metrics
                        .advance_width(glyph)
                        .ok_or_else(generation_error)?,
                },
            ))
        })
        .collect::<AppResult<_>>()?;
    Ok(PdfFont { metrics, chars })
}
struct Run {
    x: f32,
    y: f32,
    size: f32,
    text: String,
}
struct Page {
    runs: Vec<Run>,
    rules: Vec<f32>,
    detail_keys: Vec<String>,
}
fn run(page: &mut Page, x: f32, y: f32, size: f32, text: String) {
    page.runs.push(Run { x, y, size, text });
}
fn page_limit(snapshot: &ExportSnapshot) -> AppError {
    let mut error = AppError::new(
        ErrorCode::ExportTooLargeForFormat,
        "This schedule's complete PDF would exceed the PDF page limit, usually because of notes with many lines. No output was created.",
    );
    // Layout stops at the first page past the limit, so the observed count is a
    // violating lower bound, as with the assignment row limit.
    error.context.limit_kind = Some("pdf_pages");
    error.context.observed_count = Some(PAGE_LIMIT + 1);
    error.context.limit = Some(PAGE_LIMIT);
    error.context.diagnostics =
        Some(serde_json::json!({"format":"pdf","xlsx_alternative":snapshot.xlsx_available}));
    error
}
// Wrap by actual embedded font advances; split long unbroken words and preserve
// all characters. Only tab layout expands to four spaces; NFC is canonically
// equivalent rendering and never changes the DTO/XLSX text.
fn take_wrapped_line(line: &mut String) -> String {
    let remainder = match line.rfind(' ').filter(|index| *index > 0) {
        Some(index) => line.split_off(index + 1),
        None => String::new(),
    };
    std::mem::replace(line, remainder)
}
fn wrapped(font: &PdfFont, text: &str, width: f32, size: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut measured = 0.;
    for c in text.nfc().flat_map(|c| {
        if c == '\t' {
            "    ".chars().collect::<Vec<_>>()
        } else {
            vec![c]
        }
    }) {
        if c == '\n' {
            lines.push(std::mem::take(&mut line));
            measured = 0.;
            continue;
        }
        let advance = font.width(&c.to_string(), size);
        if measured + advance > width && !line.is_empty() {
            lines.push(take_wrapped_line(&mut line));
            measured = font.width(&line, size);
            if measured + advance > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                measured = 0.;
            }
        }
        line.push(c);
        measured += advance;
    }
    lines.push(line);
    lines
}
fn compact_lines(font: &PdfFont, text: &str, width: f32, size: f32, max: usize) -> Vec<String> {
    // Bounded main-table display: stop after enough text for max lines. Full
    // original values are always emitted in the assignment detail section.
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut measured = 0.;
    let mut truncated = false;
    for c in text.nfc() {
        let c = if c == '\t' { ' ' } else { c };
        let advance = if c == '\n' {
            0.
        } else {
            font.width(&c.to_string(), size)
        };
        if c == '\n' || measured + advance > width {
            if lines.len() + 1 == max {
                truncated = true;
                break;
            }
            lines.push(if c == '\n' {
                std::mem::take(&mut line)
            } else {
                take_wrapped_line(&mut line)
            });
            measured = font.width(&line, size);
            if c == '\n' {
                continue;
            }
            if measured + advance > width {
                // A long unbroken remainder has no earlier word boundary.
                // Revisit this character on the last display line by truncating
                // visibly rather than creating an extra table line.
                if lines.len() + 1 == max {
                    truncated = true;
                    break;
                }
                lines.push(std::mem::take(&mut line));
                measured = 0.;
            }
        }
        line.push(c);
        measured += advance;
    }
    if truncated {
        while font.width(&line, size) + font.width("…", size) > width {
            if line.pop().is_none() {
                break;
            }
        }
        line.push('…');
    }
    lines.push(line);
    lines
}
fn new_page(snapshot: &ExportSnapshot, font: &PdfFont, table: bool) -> Page {
    let mut page = Page {
        runs: Vec::new(),
        rules: Vec::new(),
        detail_keys: Vec::new(),
    };
    for (index, text) in snapshot.metadata().iter().enumerate() {
        run(
            &mut page,
            MARGIN,
            564. - index as f32 * 14.,
            10.,
            compact_lines(font, text, 720., 10., 1).remove(0),
        );
    }
    if table {
        let mut x = MARGIN;
        for (header, width) in COMPACT_HEADERS.iter().zip(WIDTHS) {
            for (index, text) in compact_lines(font, header, width - 6., 9., 2)
                .into_iter()
                .enumerate()
            {
                run(&mut page, x + 3., 478. - index as f32 * 12., 9., text);
            }
            x += width;
        }
        page.rules.push(464.);
    } else {
        run(
            &mut page,
            MARGIN,
            482.,
            12.,
            "Assignment details".to_owned(),
        );
    }
    page
}
// The only way pages are added, so no layout retains more than PAGE_LIMIT.
fn add_page(
    pages: &mut Vec<Page>,
    snapshot: &ExportSnapshot,
    font: &PdfFont,
    table: bool,
) -> AppResult<()> {
    if pages.len() >= PAGE_LIMIT {
        return Err(page_limit(snapshot));
    }
    pages.push(new_page(snapshot, font, table));
    Ok(())
}
fn detail_text(
    pages: &mut Vec<Page>,
    snapshot: &ExportSnapshot,
    font: &PdfFont,
    y: &mut f32,
    text: &str,
) -> AppResult<()> {
    for text in wrapped(font, text, 720., SIZE) {
        if *y < BODY_BOTTOM {
            add_page(pages, snapshot, font, false)?;
            *y = BODY_TOP;
        }
        run(
            pages.last_mut().expect("a detail page exists"),
            MARGIN,
            *y,
            SIZE,
            text,
        );
        *y -= LINE;
    }
    Ok(())
}
pub struct PdfLayout {
    pages: Vec<Page>,
}
pub fn layout(snapshot: &ExportSnapshot, font: &PdfFont) -> AppResult<PdfLayout> {
    let mut pages = Vec::new();
    add_page(&mut pages, snapshot, font, true)?;
    let mut y = BODY_TOP;
    if snapshot.rows.is_empty() {
        run(&mut pages[0], MARGIN, y, 10., "No assignments".to_owned());
    }
    for row in &snapshot.rows {
        if y - 32. < BODY_BOTTOM {
            add_page(&mut pages, snapshot, font, true)?;
            y = BODY_TOP;
        }
        let page = pages.last_mut().expect("a main page exists");
        let mut x = MARGIN;
        for (text, width) in row.compact.iter().zip(WIDTHS) {
            for (index, text) in compact_lines(font, text, width - 6., 9., 2)
                .into_iter()
                .enumerate()
            {
                run(page, x + 3., y - index as f32 * 12., 9., text);
            }
            x += width;
        }
        y -= 36.;
        page.rules.push(y + 10.);
    }
    add_page(&mut pages, snapshot, font, false)?;
    y = BODY_TOP;
    detail_text(&mut pages, snapshot, font, &mut y, "Schedule metadata")?;
    for value in snapshot.metadata() {
        detail_text(&mut pages, snapshot, font, &mut y, &value)?;
    }
    detail_text(&mut pages, snapshot, font, &mut y, "Schedule Notes:")?;
    detail_text(&mut pages, snapshot, font, &mut y, &snapshot.schedule_notes)?;
    for (index, row) in snapshot.rows.iter().enumerate() {
        if y < BODY_BOTTOM + 3. * LINE {
            add_page(&mut pages, snapshot, font, false)?;
            y = BODY_TOP;
        }
        y -= LINE;
        pages
            .last_mut()
            .expect("a detail page exists")
            .detail_keys
            .push(row.detail_key.clone());
        let label = format!(
            "Detail {} - {} / {} / {}",
            index + 1,
            row.cells[0].text(),
            row.cells[4].text(),
            row.cells[5].text()
        );
        detail_text(&mut pages, snapshot, font, &mut y, &label)?;
        // Short facts share a wrapped paragraph; full notes get their own block.
        // This keeps ordinary schedules practical to print without discarding
        // any secondary field or any visibly truncated main-table value.
        let facts = HEADERS[..19]
            .iter()
            .zip(&row.cells[..19])
            .map(|(header, cell)| format!("{header}: {}", cell.text()))
            .collect::<Vec<_>>()
            .join(" | ");
        detail_text(&mut pages, snapshot, font, &mut y, &facts)?;
        let secondary = row
            .details
            .iter()
            .map(|field| format!("{}: {}", field.label, field.value))
            .collect::<Vec<_>>()
            .join(" | ");
        detail_text(&mut pages, snapshot, font, &mut y, &secondary)?;
        detail_text(&mut pages, snapshot, font, &mut y, "Notes:")?;
        detail_text(&mut pages, snapshot, font, &mut y, &row.cells[19].text())?;
    }
    let total = pages.len();
    for (index, page) in pages.iter_mut().enumerate() {
        run(
            page,
            MARGIN,
            36.,
            9.,
            format!("Page {} of {total}", index + 1),
        );
    }
    Ok(PdfLayout { pages })
}
fn system_info() -> SystemInfo<'static> {
    SystemInfo {
        registry: Str(b"Adobe"),
        ordering: Str(b"Identity"),
        supplement: 0,
    }
}
pub fn write(font: &PdfFont, layout: &PdfLayout, temporary: &OwnedTemporary) -> AppResult<()> {
    let mut pdf = Pdf::new();
    pdf.catalog(Ref::new(1)).pages(Ref::new(2));
    let pages = (0..layout.pages.len())
        .map(|i| Ref::new(10 + i as i32 * 2))
        .collect::<Vec<_>>();
    pdf.pages(Ref::new(2))
        .kids(pages.iter().copied())
        .count(i32::try_from(pages.len()).map_err(|_| generation_error())?);
    let font_id = Ref::new(3);
    pdf.type0_font(font_id)
        .base_font(Name(b"NotoSans-Regular"))
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(Ref::new(4))
        .to_unicode(Ref::new(7));
    let mut descendant = pdf.cid_font(Ref::new(4));
    descendant
        .subtype(CidFontType::Type2)
        .base_font(Name(b"NotoSans-Regular"))
        .system_info(system_info())
        .font_descriptor(Ref::new(5))
        .cid_to_gid_map_stream(Ref::new(8));
    let scale = 1000. / f32::from(font.metrics.units_per_em);
    let mut glyph_map = vec![0u8; (font.chars.len() + 1) * 2];
    let mut cmap = UnicodeCmap::new(Name(b"ShiftHappensUnicode"), system_info());
    let mut widths = descendant.widths();
    for (&c, glyph) in &font.chars {
        let cid = glyph.cid;
        glyph_map[usize::from(cid) * 2..usize::from(cid) * 2 + 2]
            .copy_from_slice(&glyph.glyph_id.to_be_bytes());
        widths.same(cid, cid, glyph.advance * scale);
        cmap.pair(cid, c);
    }
    widths.finish();
    descendant.finish();
    let bbox = font.metrics.bounds.ok_or_else(generation_error)?;
    pdf.font_descriptor(Ref::new(5))
        .name(Name(b"NotoSans-Regular"))
        .flags(FontFlags::NON_SYMBOLIC)
        .bbox(Rect::new(
            bbox.x_min * scale,
            bbox.y_min * scale,
            bbox.x_max * scale,
            bbox.y_max * scale,
        ))
        .italic_angle(0.)
        .ascent(font.metrics.ascent * scale)
        .descent(font.metrics.descent * scale)
        .cap_height(font.metrics.cap_height.unwrap_or(font.metrics.ascent) * scale)
        .stem_v(80.)
        .font_file2(Ref::new(6));
    pdf.stream(Ref::new(6), FONT)
        .pair(Name(b"Length1"), FONT.len() as i32);
    pdf.stream(Ref::new(7), &cmap.finish());
    pdf.stream(Ref::new(8), &glyph_map);
    for (index, source) in layout.pages.iter().enumerate() {
        let page_id = pages[index];
        let content_id = Ref::new(page_id.get() + 1);
        let mut page = pdf.page(page_id);
        page.parent(Ref::new(2))
            .media_box(Rect::new(0., 0., 792., 612.))
            .contents(content_id);
        page.resources().fonts().pair(Name(b"F1"), font_id);
        // Structural detail identities are inert metadata, with no annotations,
        // actions, hyperlinks or executable content.
        page.insert(Name(b"ShiftHappensDetailKeys"))
            .array()
            .items(source.detail_keys.iter().map(|s| Str(s.as_bytes())));
        page.finish();
        let mut content = Content::new();
        for run in &source.runs {
            content
                .begin_text()
                .set_font(Name(b"F1"), run.size)
                .set_text_matrix([1., 0., 0., 1., run.x, run.y])
                .show(Str(&font.bytes(&run.text)?))
                .end_text();
        }
        content.set_stroke_gray(0.65).set_line_width(0.3);
        for &y in &source.rules {
            content.move_to(MARGIN, y).line_to(756., y).stroke();
        }
        pdf.stream(content_id, &content.finish());
    }
    let bytes = pdf.finish();
    let mut file = temporary.open_writer()?;
    file.write_all(&bytes)
        .and_then(|_| file.flush())
        .map_err(|_| generation_error())?;
    drop(file);
    Ok(())
}
// One page's text as lopdf's `extract_text` reports it, up to whitespace, for
// the operators this writer emits: lopdf decodes each Tj string through the
// page's own font resources and embedded ToUnicode CMap, and each text object
// ends with LF. Other text-showing operators and unresolved fonts are invalid.
// `extract_text` itself rebuilds the whole page map on every call, which would
// make page-by-page validation quadratic in the page count.
fn page_text(document: &lopdf::Document, page: lopdf::ObjectId, content: &[u8]) -> Option<String> {
    let encodings = document
        .get_page_fonts(page)
        .ok()?
        .into_iter()
        .map(|(name, font)| Some((name, font.get_font_encoding(document).ok()?)))
        .collect::<Option<BTreeMap<_, _>>>()?;
    let mut text = String::new();
    let mut encoding = None;
    for operation in lopdf::content::Content::decode(content).ok()?.operations {
        match operation.operator.as_str() {
            "Tf" => encoding = Some(encodings.get(operation.operands.first()?.as_name().ok()?)?),
            "Tj" => {
                for operand in &operation.operands {
                    encoding?
                        .write_to_string(operand.as_str().ok()?, &mut text)
                        .ok()?;
                }
            }
            "ET" => text.push('\n'),
            "TJ" | "'" | "\"" => return None,
            _ => {}
        }
    }
    Some(text)
}
pub fn validate(layout: &PdfLayout, temporary: &OwnedTemporary) -> AppResult<()> {
    temporary.verify()?;
    let file =
        safe_destination::open_narrow(&temporary.path, false).map_err(|_| validation_error())?;
    let document = lopdf::Document::load_from(file).map_err(|_| validation_error())?;
    // Enumerate the page tree once; pages are then checked in document order.
    let pages = document.get_pages();
    if pages.is_empty() || pages.len() != layout.pages.len() {
        return Err(validation_error());
    }
    for (id, expected) in pages.values().zip(&layout.pages) {
        let content = document.get_page_content(*id);
        if content.is_empty() {
            return Err(validation_error());
        }
        let text = page_text(&document, *id, &content).ok_or_else(validation_error)?;
        // Require the exact rendered text runs, in order, with every page footer.
        // Text objects are separated with LF; collapse only layout whitespace.
        let actual = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let expected_text = expected
            .runs
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let expected_text = expected_text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if actual != expected_text {
            return Err(validation_error());
        }
        let page = document
            .get_dictionary(*id)
            .map_err(|_| validation_error())?;
        let keys = page
            .get(b"ShiftHappensDetailKeys")
            .and_then(|v| v.as_array())
            .map_err(|_| validation_error())?;
        if keys.len() != expected.detail_keys.len() {
            return Err(validation_error());
        }
        for (actual, expected) in keys.iter().zip(&expected.detail_keys) {
            if actual.as_str().map_err(|_| validation_error())? != expected.as_bytes() {
                return Err(validation_error());
            }
        }
    }
    // All document/parser state and its consumed input handle drop on return.
    Ok(())
}
