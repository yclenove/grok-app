//! Publish every synthesized text format before capturing the receipt (v1).
use super::{data::Format, failure};
use gcu_core::driver::Result;
use windows::Win32::Globalization::*;

pub(super) fn prepare(text: &str) -> Result<Vec<Format>> {
    let unicode = Format::unicode(text)?;
    // SAFETY: scalar thread/OS locale queries, no borrowed pointers.
    let (locale, acp, oemcp) = unsafe { (GetThreadLocale(), GetACP(), GetOEMCP()) };
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    let ansi = convert(code_page(locale, LOCALE_IDEFAULTANSICODEPAGE, acp)?, &wide)?;
    let oem = convert(code_page(locale, LOCALE_IDEFAULTCODEPAGE, oemcp)?, &wide)?;
    Ok(vec![
        unicode,
        Format::new(16, locale.to_le_bytes().to_vec())?,
        Format::new(1, ansi)?,
        Format::new(7, oem)?,
    ])
}
fn code_page(locale: u32, field: u32, fallback: u32) -> Result<u32> {
    let mut value = [0u16; 16];
    // SAFETY: initialized bounded output buffer; this call reads locale metadata.
    let n = unsafe { GetLocaleInfoW(locale, field, Some(&mut value)) };
    if n <= 1 {
        return Err(failure());
    }
    let page = String::from_utf16_lossy(&value[..n as usize - 1])
        .parse::<u32>()
        .map_err(|_| failure())?;
    Ok(if page == 0 { fallback } else { page })
}
fn convert(page: u32, wide: &[u16]) -> Result<Vec<u8>> {
    // SAFETY: bounded immutable input; None output requests required byte count.
    let len = unsafe { WideCharToMultiByte(page, 0, wide, None, None, None) };
    if len <= 0 || len > 512 * 1024 {
        return Err(failure());
    }
    let mut bytes = vec![0; len as usize];
    // SAFETY: initialized output has exactly the requested size; immutable UTF-16
    // input remains alive throughout the conversion. Default substitution allowed
    // for ANSI/OEM; Unicode remains the authoritative paste format.
    if unsafe { WideCharToMultiByte(page, 0, wide, Some(&mut bytes), None, None) } != len {
        return Err(failure());
    }
    Ok(bytes)
}
