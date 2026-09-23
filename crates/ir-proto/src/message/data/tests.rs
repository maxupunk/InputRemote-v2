use super::*;

fn file(path: &str, size: u64) -> ManifestItem {
    ManifestItem {
        path: path.to_owned(),
        size,
        is_dir: false,
    }
}

fn dir(path: &str) -> ManifestItem {
    ManifestItem {
        path: path.to_owned(),
        size: 0,
        is_dir: true,
    }
}

#[test]
fn ordinary_relative_paths_are_safe() {
    for path in [
        "a.txt",
        "pasta/a.txt",
        "a/b/c/d.bin",
        "com espaço.txt",
        "acentuação.md",
    ] {
        assert!(file(path, 1).is_safe_path(), "{path:?} deveria ser seguro");
    }
}

#[test]
fn directory_traversal_is_refused() {
    for path in [
        "../fora.txt",
        "a/../../fora.txt",
        "..",
        "a/..",
        "./a.txt",
        "a/./b",
    ] {
        assert!(
            !file(path, 1).is_safe_path(),
            "{path:?} deveria ser recusado"
        );
    }
}

#[test]
fn absolute_and_windows_paths_are_refused() {
    for path in [
        "/etc/passwd",
        "C:/Windows/System32/x.dll",
        "c:x",
        "a\\b",
        "\\\\servidor\\x",
    ] {
        assert!(
            !file(path, 1).is_safe_path(),
            "{path:?} deveria ser recusado"
        );
    }
}

#[test]
fn a_drive_or_stream_in_any_component_is_refused() {
    // `x/C:payload.dll` escapava: `PathBuf::push` troca a raiz inteira por `C:`.
    for path in [
        "x/C:payload.dll",
        "a/b/c:x",
        "a/arquivo.txt:fluxo",
        "a:b",
        "pasta/::$DATA",
    ] {
        assert!(
            !file(path, 1).is_safe_path(),
            "{path:?} deveria ser recusado"
        );
    }
}

#[test]
fn windows_device_names_are_refused_with_or_without_extension() {
    for path in [
        "CON",
        "nul",
        "a/NUL.txt",
        "Com1",
        "lpt9.log",
        "aux",
        "prn.x",
        "a/CONIN$",
        "COM¹",
    ] {
        assert!(
            !file(path, 1).is_safe_path(),
            "{path:?} deveria ser recusado"
        );
    }
    for path in ["console.txt", "com10", "nula.txt", "lpt", "a/auxiliar"] {
        assert!(file(path, 1).is_safe_path(), "{path:?} é um nome comum");
    }
}

#[test]
fn names_windows_would_rewrite_or_reject_are_refused() {
    for path in [
        "a.",
        "a ",
        "p./x",
        "a/b?",
        "a*",
        "a|b",
        "a<b",
        "a>b",
        "a\"b",
        "a\u{7}b",
        "linha\nnova",
    ] {
        assert!(
            !file(path, 1).is_safe_path(),
            "{path:?} deveria ser recusado"
        );
    }
}

#[test]
fn empty_null_and_oversized_paths_are_refused() {
    assert!(!file("", 1).is_safe_path());
    assert!(!file("a\0b", 1).is_safe_path());
    assert!(!file("a//b", 1).is_safe_path(), "componente vazio");
    let long = "a".repeat(limits::MAX_RELATIVE_PATH + 1);
    assert!(!file(&long, 1).is_safe_path());
}

#[test]
fn a_consistent_manifest_validates() {
    let items = vec![dir("pasta"), file("pasta/a.txt", 10), file("b.bin", 32)];
    assert!(validate_manifest(&items, 42).is_ok());
}

#[test]
fn directories_do_not_count_towards_the_total() {
    let items = vec![dir("pasta"), dir("pasta/sub")];
    assert!(validate_manifest(&items, 0).is_ok());
}

#[test]
fn a_lying_total_is_refused() {
    let items = vec![file("a.txt", 10)];
    assert_eq!(
        validate_manifest(&items, 999).unwrap_err(),
        ProtoError::Malformed
    );
}

#[test]
fn an_overflowing_total_is_refused_without_panicking() {
    let items = vec![file("a.txt", u64::MAX), file("b.txt", 2)];
    assert_eq!(
        validate_manifest(&items, 1).unwrap_err(),
        ProtoError::Malformed
    );
}

#[test]
fn one_unsafe_path_rejects_the_whole_manifest() {
    let items = vec![file("bom.txt", 1), file("../mau.txt", 1)];
    assert_eq!(
        validate_manifest(&items, 2).unwrap_err(),
        ProtoError::Malformed
    );
}

#[test]
fn item_count_is_checked_before_walking_the_list() {
    let items = vec![file("../mau.txt", 0); limits::MAX_MANIFEST_ITEMS + 1];
    let err = validate_manifest(&items, 0).unwrap_err();
    assert!(matches!(
        err,
        ProtoError::CountTooLarge {
            what: "itens do manifesto",
            ..
        }
    ));
}
