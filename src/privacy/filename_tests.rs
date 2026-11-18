#[test]
fn filename_aliases_are_not_issued() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("real-name.txt"), "x").unwrap();
    let mut names =
        super::filenames::FilenameAliases::new(temp.path().to_path_buf(), "preview").unwrap();
    assert_eq!(
        names.project_text("real-name.txt").unwrap(),
        "real-name.txt"
    );
}
