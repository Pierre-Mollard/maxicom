//! The named pipe's permissions on Windows (MAX-74): the user who runs maxicom, nobody else.
//! A pipe's default lets every user read it; Unix gets the same from the socket file's 0600
use std::{io, ptr};

use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use widestring::{U16CStr, U16CString};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, LocalFree},
    Security::{
        Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

/// The pipe's security descriptor: full access for this user alone (`P`: nothing inherited).
/// Clients from another machine are refused anyway, `interprocess`'s default
pub fn owner_only() -> io::Result<SecurityDescriptor> {
    let sddl = owner_only_sddl(&user_sid()?);
    let sddl = U16CString::from_str(&sddl).map_err(io::Error::other)?;
    SecurityDescriptor::deserialize(&sddl)
}

/// The descriptor as text, in Windows' SDDL: one ACE allowing (`A`) everything (`GA`) to `sid`
pub fn owner_only_sddl(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})")
}

/// The SID of the user running this process, as text (`S-1-5-21-…`)
pub fn user_sid() -> io::Result<String> {
    let mut token: HANDLE = ptr::null_mut();
    // SAFETY: the current process' pseudo handle, `token` written on success
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let user = token_user(token);
    // SAFETY: opened above, closed once
    unsafe { CloseHandle(token) };
    let buffer = user?;
    // SAFETY: `buffer` holds a TOKEN_USER, u64 aligned, its SID inside the same buffer
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut text = ptr::null_mut();
    // SAFETY: a valid SID; `text` is allocated by the call, freed below
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a nul-terminated string from the call above
    let sid = unsafe { U16CStr::from_ptr_str(text) }.to_string_lossy();
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc
    unsafe { LocalFree(text.cast()) };
    Ok(sid)
}

/// The token's user, in a buffer of the size the first call asks for
fn token_user(token: HANDLE) -> io::Result<Vec<u64>> {
    let mut size = 0u32;
    // SAFETY: a size query, no buffer: it fails and gives the size needed
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size) };
    if size == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: `buffer` holds `size` bytes at least
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(buffer)
}

#[cfg(test)]
#[path = "../tests/api/windows.rs"]
mod tests;
