use crate::{
    error::{AppError, AppResult, ErrorCode},
    export_snapshot::{ExportCell, ExportSnapshot, HEADERS},
    safe_destination::{self, OwnedTemporary},
};
use calamine::{Data, Reader, Xlsx};
use rust_xlsxwriter::{Format, FormatAlign, Workbook};

pub const TABLE_HEADER_ROW: u32 = 7;
pub fn generation_error() -> AppError {
    AppError::new(
        ErrorCode::ExportGenerationFailed,
        "The local document could not be generated. No output was published.",
    )
}
pub fn validation_error() -> AppError {
    AppError::new(
        ErrorCode::ExportValidationFailed,
        "Independent document validation failed. No output was published.",
    )
}
pub fn preflight(snapshot: &ExportSnapshot) -> AppResult<()> {
    if !snapshot.xlsx_available {
        return Err(AppError::new(
            ErrorCode::XlsxTextLimitExceeded,
            "A cell exceeds Excel's 32,767 UTF-16-unit text limit. Shorten the text before exporting to XLSX.",
        ));
    }
    Ok(())
}
pub fn write(snapshot: &ExportSnapshot, temporary: &OwnedTemporary) -> AppResult<()> {
    let mut workbook = Workbook::new();
    // Constant memory scratch is backend-only, owned by tempfile and closed by
    // Workbook drop. Keep it in the selected folder rather than a shared temp dir.
    workbook
        .set_tempdir(temporary.path.parent().ok_or_else(generation_error)?)
        .map_err(|_| generation_error())?;
    let worksheet = workbook.add_worksheet_with_constant_memory();
    worksheet
        .set_name("Schedule")
        .map_err(|_| generation_error())?;
    let normal = Format::new().set_text_wrap().set_align(FormatAlign::Top);
    let heading = Format::new()
        .set_bold()
        .set_text_wrap()
        .set_background_color("#E2E8F0")
        .set_font_color("#000000");
    for (row, value) in snapshot.metadata().iter().enumerate() {
        worksheet
            .write_string_with_format(row as u32, 0, value, &normal)
            .map_err(|_| generation_error())?;
    }
    for (column, value) in HEADERS.iter().enumerate() {
        worksheet
            .write_string_with_format(TABLE_HEADER_ROW, column as u16, *value, &heading)
            .map_err(|_| generation_error())?;
    }
    worksheet
        .set_row_height(TABLE_HEADER_ROW, 32)
        .map_err(|_| generation_error())?;
    worksheet
        .set_freeze_panes(TABLE_HEADER_ROW + 1, 0)
        .map_err(|_| generation_error())?;
    for (column, width) in [
        14, 16, 24, 14, 26, 12, 24, 36, 15, 24, 24, 28, 12, 16, 14, 16, 10, 14, 18, 60,
    ]
    .iter()
    .enumerate()
    {
        worksheet
            .set_column_width(column as u16, *width)
            .map_err(|_| generation_error())?;
    }
    for (index, row) in snapshot.rows.iter().enumerate() {
        let row_number = TABLE_HEADER_ROW + 1 + index as u32;
        for (column, cell) in row.cells.iter().enumerate() {
            // Explicit literal string API never recognizes formulas or URLs.
            match cell {
                ExportCell::Text(value) => {
                    worksheet
                        .write_string_with_format(row_number, column as u16, value, &normal)
                        .map_err(|_| generation_error())?;
                }
                ExportCell::Integer(value) => {
                    worksheet
                        .write_number(row_number, column as u16, *value as f64)
                        .map_err(|_| generation_error())?;
                }
            }
        }
    }
    let file = temporary.open_writer()?;
    workbook
        .save_to_writer(file)
        .map_err(|_| generation_error())?;
    // save_to_writer finalizes the ZIP and closes its owned file. Drop scratch
    // streams before the caller's exact-file sync and independent validation.
    drop(workbook);
    Ok(())
}
pub fn validate(snapshot: &ExportSnapshot, temporary: &OwnedTemporary) -> AppResult<()> {
    temporary.verify()?;
    let file =
        safe_destination::open_narrow(&temporary.path, false).map_err(|_| validation_error())?;
    let mut reader = Xlsx::new(file).map_err(|_| validation_error())?;
    if reader.sheet_names() != ["Schedule"] {
        return Err(validation_error());
    }
    let range = reader
        .worksheet_range("Schedule")
        .map_err(|_| validation_error())?;
    let formulas = reader
        .worksheet_formula("Schedule")
        .map_err(|_| validation_error())?;
    if formulas.cells().any(|(_, _, value)| !value.is_empty()) {
        return Err(validation_error());
    }
    for (row, value) in snapshot.metadata().iter().enumerate() {
        if range.get_value((row as u32, 0)) != Some(&Data::String(value.clone())) {
            return Err(validation_error());
        }
    }
    for (column, value) in HEADERS.iter().enumerate() {
        if range.get_value((TABLE_HEADER_ROW, column as u32))
            != Some(&Data::String((*value).to_owned()))
        {
            return Err(validation_error());
        }
    }
    if range.height() != TABLE_HEADER_ROW as usize + 1 + snapshot.rows.len()
        || range.width() != HEADERS.len()
    {
        return Err(validation_error());
    }
    for (index, row) in snapshot.rows.iter().enumerate() {
        for (column, cell) in row.cells.iter().enumerate() {
            let observed = range.get_value((TABLE_HEADER_ROW + 1 + index as u32, column as u32));
            let matches = match cell {
                ExportCell::Text(value) if value.is_empty() => {
                    matches!(observed, None | Some(Data::Empty))
                        || observed == Some(&Data::String(String::new()))
                }
                ExportCell::Text(value) => observed == Some(&Data::String(value.clone())),
                ExportCell::Integer(value) => {
                    observed == Some(&Data::Int(*value))
                        || observed == Some(&Data::Float(*value as f64))
                }
            };
            if !matches {
                return Err(validation_error());
            }
        }
    }
    // Reader, ranges, ZIP reader, and owned read handle all drop before publish.
    Ok(())
}
