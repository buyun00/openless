pub struct Anchor {
    prefix: String,
    suffix: String,
}
impl Anchor {
    pub fn new(document: &str, inserted: &str) -> Option<Self> {
        if inserted.is_empty() {
            return None;
        }
        let at = document.find(inserted)?;
        if document.rfind(inserted) != Some(at) {
            return None;
        }
        Some(Self {
            prefix: document[..at].into(),
            suffix: document[at + inserted.len()..].into(),
        })
    }
    pub fn extract(&self, document: &str) -> Option<String> {
        let middle = document
            .strip_prefix(&self.prefix)?
            .strip_suffix(&self.suffix)?;
        (middle.chars().count() <= 4000).then(|| middle.to_string())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tracks_only_inserted_region() {
        let a = Anchor::new("前文你好后文", "你好").unwrap();
        assert_eq!(a.extract("前文您好后文"), Some("您好".into()));
        assert!(a.extract("改了前文您好后文").is_none());
    }
    #[test]
    fn rejects_ambiguous_and_missing_baseline() {
        assert!(Anchor::new("你好你好", "你好").is_none());
        assert!(Anchor::new("别的文字", "你好").is_none());
        assert!(Anchor::new("文字", "").is_none());
    }
    #[test]
    fn supports_unicode_and_insertion() {
        let a = Anchor::new("[🚀大疆]", "🚀大疆").unwrap();
        assert_eq!(a.extract("[🚀大疆麦克风]"), Some("🚀大疆麦克风".into()));
    }
    #[test]
    fn handles_empty_and_oversized_edits() {
        let a = Anchor::new("[文字]", "文字").unwrap();
        assert_eq!(a.extract("[]"), Some(String::new()));
        assert!(a.extract(&format!("[{}]", "字".repeat(4001))).is_none());
    }
}
