//! Secrets at rest: saved passwords and key passphrases.
//!
//! Sealed with DPAPI, which ties them to this Windows account on this machine.
//! Nothing is asked for and no key is kept: Windows holds it, so a copy of the
//! file taken elsewhere is noise.

use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};

use crate::{Error, Result};

/// Never show a prompt: this runs behind a connection being made.
const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    }
}

/// Copy a result out and hand the OS its buffer back.
///
/// # Safety
/// `out` must be a blob DPAPI returned and that has not been freed.
unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    let owned = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
    // DPAPI allocates with LocalAlloc; skipping this leaks on every call.
    let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
    owned
}

fn protect(secret: &[u8]) -> Result<Vec<u8>> {
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: the input blob borrows `secret` for the call; the output is
    // DPAPI's to fill and `take`'s to free.
    unsafe {
        CryptProtectData(
            &blob(secret),
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| Error::Other(anyhow::anyhow!("could not encrypt the secret: {e}")))?;
        Ok(take(output))
    }
}

fn unprotect(sealed: &[u8]) -> Result<Vec<u8>> {
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: as in `protect`.
    unsafe {
        CryptUnprotectData(
            &blob(sealed),
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| {
            Error::Other(anyhow::anyhow!(
                "could not decrypt the stored secret; it may belong to a different \
                 Windows account or machine: {e}"
            ))
        })?;
        Ok(take(output))
    }
}

/// Hex rather than base64: a dependency saved, for a handful of small blobs.
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(text: &str) -> Result<Vec<u8>> {
    let malformed = || Error::Other(anyhow::anyhow!("the stored secret is malformed"));
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err(malformed());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| malformed()))
        .collect()
}

/// Encrypt a secret into text that can sit in a JSON file.
pub fn seal(secret: &str) -> Result<String> {
    Ok(to_hex(&protect(secret.as_bytes())?))
}

/// The reverse of [`seal`].
pub fn unseal(sealed: &str) -> Result<String> {
    String::from_utf8(unprotect(&from_hex(sealed)?)?)
        .map_err(|_| Error::Other(anyhow::anyhow!("the stored secret is not text")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_comes_back_as_it_went_in() {
        let sealed = seal("correct horse battery staple").unwrap();
        assert!(
            !sealed.contains("horse"),
            "sealed text must not be readable"
        );
        assert_eq!(unseal(&sealed).unwrap(), "correct horse battery staple");
    }

    #[test]
    fn an_empty_secret_is_a_secret_too() {
        assert_eq!(unseal(&seal("").unwrap()).unwrap(), "");
    }

    #[test]
    fn something_that_was_never_sealed_is_refused() {
        assert!(unseal("not hex").is_err());
        assert!(unseal("abc").is_err());
        assert!(unseal("00112233").is_err());
    }
}
