use std::path::{Component, Path};

#[cfg(windows)]
fn names_equal(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringOrdinal};
    let a: Vec<_> = a.encode_wide().collect();
    let b: Vec<_> = b.encode_wide().collect();
    let (Ok(a_len), Ok(b_len)) = (i32::try_from(a.len()), i32::try_from(b.len())) else {
        return false;
    };
    // Compare UTF-16 without replacing unpaired surrogates or applying the
    // user's locale. Windows path case folding includes non-ASCII letters.
    unsafe { CompareStringOrdinal(a.as_ptr(), a_len, b.as_ptr(), b_len, 1) == CSTR_EQUAL }
}

fn components_equal(a: Component<'_>, b: Component<'_>) -> bool {
    #[cfg(windows)]
    {
        use std::path::Prefix;
        match (a, b) {
            (Component::Normal(a), Component::Normal(b)) => names_equal(a, b),
            (Component::Prefix(a), Component::Prefix(b)) => match (a.kind(), b.kind()) {
                (Prefix::Disk(a), Prefix::Disk(b))
                | (Prefix::VerbatimDisk(a), Prefix::VerbatimDisk(b)) => a.eq_ignore_ascii_case(&b),
                (Prefix::UNC(a_server, a_share), Prefix::UNC(b_server, b_share))
                | (
                    Prefix::VerbatimUNC(a_server, a_share),
                    Prefix::VerbatimUNC(b_server, b_share),
                ) => names_equal(a_server, b_server) && names_equal(a_share, b_share),
                (Prefix::DeviceNS(a), Prefix::DeviceNS(b))
                | (Prefix::Verbatim(a), Prefix::Verbatim(b)) => names_equal(a, b),
                _ => false,
            },
            (Component::RootDir, Component::RootDir) | (Component::CurDir, Component::CurDir) => {
                true
            }
            _ => false,
        }
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

pub(crate) fn same_or_child(path: &Path, parent: &Path) -> bool {
    let mut parts = path.components();
    parent.components().all(|expected| {
        parts
            .next()
            .is_some_and(|actual| components_equal(actual, expected))
    })
}

pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    a.components().count() == b.components().count() && same_or_child(a, b)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn unicode_case_and_separator_aliases_compare_without_losing_utf16() {
        use std::{ffi::OsString, os::windows::ffi::OsStringExt};
        assert!(same_path(
            Path::new(r"C:\Données\cache"),
            Path::new("c:/DONNÉES/CACHE")
        ));
        let a = OsString::from_wide(&[0xd800]);
        let b = OsString::from_wide(&[0xd801]);
        assert!(names_equal(&a, &a));
        assert!(!names_equal(&a, &b));
        assert!(!same_path(Path::new(r"C:\app"), Path::new(r"\\?\C:\app")));
        assert!(!same_path(
            Path::new(r"C:\app"),
            Path::new(r"C:\other\..\app")
        ));
    }
}
