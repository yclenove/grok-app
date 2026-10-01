//! Prepare all four text formats before acquiring/mutating the clipboard. With
//! these present CloseClipboard has no implicit text formats left to synthesize.
use super::data::{OwnedFormat, MAX_BYTES};
use windows::Win32::Globalization::{
    GetACP, GetLocaleInfoW, GetOEMCP, GetThreadLocale, WideCharToMultiByte,
    LOCALE_IDEFAULTANSICODEPAGE, LOCALE_IDEFAULTCODEPAGE,
};

pub(super) fn prepare(text: &str) -> Result<Vec<OwnedFormat>, String> {
    let unicode = OwnedFormat::text(text)?;
    let locale = unsafe { GetThreadLocale() };
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let ansi_page = code_page(locale, LOCALE_IDEFAULTANSICODEPAGE, unsafe { GetACP() })?;
    let oem_page = code_page(locale, LOCALE_IDEFAULTCODEPAGE, unsafe { GetOEMCP() })?;
    let ansi = convert(ansi_page, &wide)?;
    let oem = convert(oem_page, &wide)?;
    if wide.len() * 2 + ansi.len() + oem.len() + 4 > MAX_BYTES {
        return Err("clipboard text formats exceed their combined budget".into());
    }
    Ok(vec![
        unicode,
        OwnedFormat::bytes(16, &locale.to_le_bytes())?,
        OwnedFormat::bytes(1, &ansi)?,
        OwnedFormat::bytes(7, &oem)?,
    ])
}

fn code_page(locale: u32, field: u32, fallback: u32) -> Result<u32, String> {
    let mut value = [0u16; 16];
    let n = unsafe { GetLocaleInfoW(locale, field, Some(&mut value)) };
    if n <= 1 {
        return Err("clipboard locale code page is unavailable".into());
    }
    let number = String::from_utf16_lossy(&value[..n as usize - 1])
        .parse::<u32>()
        .map_err(|_| "clipboard locale code page is invalid")?;
    Ok(if number == 0 { fallback } else { number })
}

fn convert(code_page: u32, wide: &[u16]) -> Result<Vec<u8>, String> {
    let len = unsafe { WideCharToMultiByte(code_page, 0, wide, None, None, None) };
    if len <= 0 || len as usize > MAX_BYTES {
        return Err("clipboard text conversion exceeds its budget".into());
    }
    let mut bytes = vec![0u8; len as usize];
    let written = unsafe { WideCharToMultiByte(code_page, 0, wide, Some(&mut bytes), None, None) };
    if written != len {
        return Err("clipboard text conversion did not complete".into());
    }
    Ok(bytes)
}
