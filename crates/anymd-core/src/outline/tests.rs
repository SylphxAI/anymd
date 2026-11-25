use super::{headings, noun_for_units, DocumentInfo, Outline, Unit};

#[test]
fn headings_ignore_code_and_keep_utf8_offsets() {
    let text = "# 中文\n\n```md\n# not a heading\n```\n\n<!--\n# hidden\n-->\n\nRevenue\n-------\n## Costs ##\n";
    let found = headings(text);
    assert_eq!(
        found.iter().map(|h| h.1.as_str()).collect::<Vec<_>>(),
        ["中文", "Revenue", "Costs"]
    );
    for (_, title, start) in found {
        assert!(text[start..].contains(&title));
    }
}

#[test]
fn outline_ranges_align_with_the_utf8_canonical_body() {
    let units = [
        Unit {
            number: 1,
            label: "chapter 一".into(),
            markdown: "\u{3000}# 中文\n\nRésumé 😀\n".into(),
        },
        Unit {
            number: 2,
            label: "chapter 二".into(),
            markdown: "# 二\n\n## 日本語\n\n本文".into(),
        },
    ];
    let info = DocumentInfo {
        source: "book.epub",
        format: "epub",
        title: None,
        unit_noun: noun_for_units(&units, false),
        total: 2,
        paged: false,
    };
    assert_eq!(info.unit_noun, "chapter");
    let outline = Outline::from_units(info, &units, &[]);
    let again = Outline::from_units(info, &units, &[]);
    assert_eq!(outline.tree(), again.tree());
    assert_eq!(
        serde_json::to_value(&outline).unwrap(),
        serde_json::to_value(&again).unwrap()
    );
    let canonical = "<!-- chapter 一 -->\n\n# 中文\n\nRésumé 😀\n\n<!-- chapter 二 -->\n\n# 二\n\n## 日本語\n\n本文\n\n";
    assert_eq!(outline.nodes[0].end, canonical.len());
    for (range, unit) in outline.units.iter().zip(&units) {
        assert_eq!(&canonical[range.start..range.end], unit.markdown.trim());
    }
    for node in &outline.nodes {
        assert!(node.start <= node.end && node.end <= canonical.len());
        assert!(canonical.is_char_boundary(node.start) && canonical.is_char_boundary(node.end));
    }
    assert_eq!(outline.node_at(1, 3).title, "中文");
    let subheading = outline.node_at(2, "# 二\n\n".len());
    assert_eq!(subheading.path, "二 > 日本語");
    assert_eq!(
        &canonical[subheading.start..subheading.end],
        "## 日本語\n\n本文"
    );
}
