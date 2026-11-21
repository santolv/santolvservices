use std::{io, path::Path};
pub fn private_directory(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    windows_acl(path)?;
    Ok(())
}
#[cfg(windows)]
fn windows_acl(path: &Path) -> io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SE_FILE_OBJECT,
                SetNamedSecurityInfoW,
            },
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
            PROTECTED_DACL_SECURITY_INFORMATION,
        },
    };
    // Owner Rights grants access to the actual owner; SYSTEM permits OS-managed execution.
    let sddl = "D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)\0"
        .encode_utf16()
        .collect::<Vec<_>>();
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    unsafe {
        let mut descriptor = ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = ptr::null_mut();
        if GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted) == 0
            || present == 0
            || dacl.is_null()
        {
            let error = io::Error::last_os_error();
            LocalFree(descriptor);
            return Err(error);
        }
        let code = SetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        );
        LocalFree(descriptor);
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn private_directory_stays_usable() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("private");
        super::private_directory(&path).unwrap();
        let file = path.join("secret");
        std::fs::write(&file, b"value").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"value");
        super::private_directory(&path).unwrap();
    }
}
