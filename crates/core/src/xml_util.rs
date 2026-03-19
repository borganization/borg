/// Escape a string for safe embedding in XML attribute values.
pub fn escape_xml_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_all_special_chars() {
        assert_eq!(
            escape_xml_attr("a & b < c > d \" e ' f"),
            "a &amp; b &lt; c &gt; d &quot; e &apos; f"
        );
    }

    #[test]
    fn no_special_chars_unchanged() {
        assert_eq!(escape_xml_attr("hello world"), "hello world");
    }

    #[test]
    fn empty_string() {
        assert_eq!(escape_xml_attr(""), "");
    }

    #[test]
    fn ampersand_escaped_first() {
        // Ensures & is escaped before other replacements that produce &
        assert_eq!(escape_xml_attr("&"), "&amp;");
        assert_eq!(escape_xml_attr("&quot;"), "&amp;quot;");
    }
}
