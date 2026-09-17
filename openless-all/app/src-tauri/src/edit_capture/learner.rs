//! Conservative single-span word candidates. No model calls or sentence rewriting.
pub fn candidate(before: &str, after: &str) -> Option<(String, String)> {
    let a: Vec<char> = before.trim().chars().collect();
    let b: Vec<char> = after.trim().chars().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let mut start = prefix;
    let mut end_a = a.len() - suffix;
    let mut end_b = b.len() - suffix;
    // Keep Latin tokens whole (e.g. openles -> openless), not just changed letters.
    if a.get(start).is_some_and(char::is_ascii_alphabetic)
        || b.get(start).is_some_and(char::is_ascii_alphabetic)
    {
        while start > 0 && a[start - 1].is_ascii_alphabetic() {
            start -= 1;
        }
        while end_a < a.len() && a[end_a].is_ascii_alphabetic() {
            end_a += 1;
        }
        while end_b < b.len() && b[end_b].is_ascii_alphabetic() {
            end_b += 1;
        }
    }
    let source: String = a[start..end_a].iter().collect();
    let target: String = b[start..end_b].iter().collect();
    let valid = |word: &str| {
        (2..=12).contains(&word.chars().count())
            && word.chars().all(char::is_alphabetic)
            && ![
                "今天", "明天", "昨天", "后天", "下周", "本周", "下月", "本月", "今年", "明年",
                "上午", "下午", "晚上", "早上", "之前", "之后", "不是", "不要", "可以", "不能",
            ]
            .iter()
            .any(|term| word.contains(term))
            && !word
                .chars()
                .any(|c| "零一二三四五六七八九十百千万亿年月日点分秒".contains(c))
    };
    (source != target && valid(&source) && valid(&target)).then_some((source, target))
}

/// The two latest relevant *independent sessions* must agree. A different target
/// for the same source or a reverse edit breaks the streak.
pub fn streak<'a>(
    source: &str,
    target: &str,
    pairs: impl Iterator<Item = &'a (String, String)>,
) -> usize {
    let mut count = 0;
    for (from, to) in pairs {
        if from == source {
            if to != target {
                break;
            }
            count += 1;
        } else if from == target && to == source {
            break;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_word_inside_chinese_sentence() {
        assert_eq!(
            candidate("我在青语 聊天。", "我在轻羽 聊天。"),
            Some(("青语".into(), "轻羽".into()))
        );
    }
    #[test]
    fn latin_words_remain_whole() {
        assert_eq!(
            candidate("use openles now", "use openless now"),
            Some(("openles".into(), "openless".into()))
        );
    }
    #[test]
    fn ignores_non_word_changes() {
        for (a, b) in [
            ("青语", "青语 "),
            ("明天交付", "下周交付"),
            ("付100元", "付200元"),
            ("你好", ""),
            ("", "轻羽"),
            ("甲，乙。", "丙，丁。"),
            ("大江", "大疆"),
        ] {
            assert!(candidate(a, b).is_none(), "{a} -> {b}");
        }
    }
    #[test]
    fn conflicts_break_streak() {
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            streak(
                "青语",
                "轻羽",
                [pair("青语", "轻羽"), pair("青语", "轻羽")].iter()
            ),
            2
        );
        assert_eq!(
            streak(
                "青语",
                "轻羽",
                [
                    pair("青语", "轻羽"),
                    pair("轻羽", "青语"),
                    pair("青语", "轻羽")
                ]
                .iter()
            ),
            1
        );
        assert_eq!(
            streak(
                "青语",
                "轻羽",
                [
                    pair("青语", "轻羽"),
                    pair("青语", "清雨"),
                    pair("青语", "轻羽")
                ]
                .iter()
            ),
            1
        );
    }
}
