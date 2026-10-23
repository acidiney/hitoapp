pub struct AudioFile {
    pub file: std::fs::File,
    pub length: u64,
    pub content_type: &'static str,
}
