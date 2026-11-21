use super::render::defuse;
use super::spans::{tree, Node, Token};
use super::{escape, join_changed, join_marked, splice, splice_len, Change, Critic, Mark, Marked};
use Mark::{Deletion as Del, Highlight as Hl, Insertion as Ins};

fn text(critic: &mut Critic, t: &str) {
    critic.push(t, false, false, None);
}

fn md(critic: Critic) -> String {
    critic.into_inline().render(true)
}

/// Builds a paragraph from `(op, value)` steps: `open`/`close` take `+`,
/// `-` or `=`; `note` is a comment, `raw` raw Markdown, anything else text.
fn script(steps: &[(&str, &str)]) -> String {
    let mut critic = Critic::default();
    for (op, value) in steps {
        match *op {
            "open" => critic.open(kind(value), None),
            // `by+Ana (d)`: an insertion or deletion made by Ana at d.
            op if op.starts_with("by") => critic.open(kind(&op[2..]), Some(value)),
            "close" => critic.close(kind(value)),
            "note" => critic.comment(value),
            "raw" => critic.raw(value),
            _ => text(&mut critic, value),
        }
    }
    md(critic)
}

fn kind(value: &str) -> Mark {
    match value {
        "+" => Ins,
        "-" => Del,
        _ => Hl,
    }
}

#[test]
fn bounded_join_refuses_before_extending_and_counts_marked_separators() {
    let mut joined = Marked::new(" ", 5);
    joined.push(("abc".into(), None, (false, false))).unwrap();
    assert!(joined.push(("de".into(), None, (false, false))).is_err());
    assert_eq!(joined.finish(), "abc");
    for change in [None, Some((Ins, None)), Some((Del, Some("Author".into())))] {
        for before in ["a", "{++a++}", "{--a--}{>>Author<<}"] {
            for after in ["b", "{++b++}", "{--b--}{>>Author<<}"] {
                let size = splice_len(before, "\n\n", "# ", after, change.as_ref(), true);
                let mut out = before.to_string();
                splice(&mut out, "\n\n", "# ", after, change.as_ref(), true);
                assert_eq!(size, out.len());
            }
        }
    }
}

#[test]
fn plain_marks_render_with_their_delimiters() {
    assert_eq!(
        script(&[
            ("open", "+"),
            ("t", "a"),
            ("close", "+"),
            ("t", " "),
            ("open", "="),
            ("t", "b"),
            ("close", "=")
        ]),
        "{++a++} {==b==}"
    );
    assert_eq!(
        script(&[("open", "-"), ("t", "gone"), ("close", "-")]),
        "{--gone--}"
    );
}

#[test]
fn deletion_next_to_insertion_becomes_a_substitution_in_either_order() {
    let del_ins = [
        ("open", "-"),
        ("t", "old"),
        ("close", "-"),
        ("open", "+"),
        ("t", "new"),
        ("close", "+"),
    ];
    let ins_del = [
        ("open", "+"),
        ("t", "new"),
        ("close", "+"),
        ("open", "-"),
        ("t", "old"),
        ("close", "-"),
    ];
    assert_eq!(script(&del_ins), "{~~old~>new~~}");
    assert_eq!(script(&ins_del), "{~~old~>new~~}");
    // Only one pair: a third span stays on its own.
    let triple = [
        ("open", "-"),
        ("t", "a"),
        ("close", "-"),
        ("open", "+"),
        ("t", "b"),
        ("close", "+"),
        ("open", "-"),
        ("t", "c"),
        ("close", "-"),
    ];
    assert_eq!(script(&triple), "{~~a~>b~~}{--c--}");
}

#[test]
fn same_kind_nesting_is_merged_into_one_span() {
    let nested = [
        ("open", "-"),
        ("t", "a"),
        ("open", "-"),
        ("t", "b"),
        ("close", "-"),
        ("t", "c"),
        ("close", "-"),
        ("t", "d"),
    ];
    // The outer close still ends the span: `d` is outside it.
    assert_eq!(script(&nested), "{--abc--}d");
}

#[test]
fn merged_spans_are_counted_per_kind() {
    // Two overlapping highlights, and an insertion inside them: closing the
    // insertion must not use up the highlights' count.
    let steps = [
        ("open", "="),
        ("open", "="),
        ("t", "a"),
        ("open", "+"),
        ("t", "b"),
        ("close", "+"),
        ("t", "c"),
        ("close", "="),
        ("close", "="),
    ];
    assert_eq!(script(&steps), "{==a{++b++}c==}");
}

#[test]
fn different_kinds_may_nest() {
    // Inserted by one author, deleted by another: gone either way.
    let nested = [
        ("open", "+"),
        ("open", "-"),
        ("t", "x"),
        ("close", "-"),
        ("close", "+"),
    ];
    assert_eq!(script(&nested), "{++{--x--}++}");
}

#[test]
fn crossing_spans_are_split_so_each_closes_inside_its_parent() {
    let crossing = [
        ("open", "="),
        ("t", "a"),
        ("open", "+"),
        ("t", "b"),
        ("close", "="),
        ("t", "c"),
        ("close", "+"),
    ];
    assert_eq!(script(&crossing), "{==a{++b++}==}{++c++}");
}

#[test]
fn neighbours_of_the_same_kind_join() {
    let split = [
        ("open", "+"),
        ("t", "a"),
        ("close", "+"),
        ("open", "+"),
        ("t", "b"),
        ("close", "+"),
    ];
    assert_eq!(script(&split), "{++ab++}");
    // Joining exposes inner neighbours, which join too.
    let deep = [
        ("open", "-"),
        ("open", "+"),
        ("t", "a"),
        ("close", "+"),
        ("close", "-"),
        ("open", "-"),
        ("open", "+"),
        ("t", "b"),
        ("close", "+"),
        ("close", "-"),
    ];
    assert_eq!(script(&deep), "{--{++ab++}--}");
}

#[test]
fn empty_spans_vanish_but_keep_their_comments() {
    assert_eq!(
        script(&[("t", "a"), ("open", "+"), ("close", "+"), ("t", "b")]),
        "ab"
    );
    assert_eq!(
        script(&[
            ("t", "a"),
            ("open", "+"),
            ("open", "-"),
            ("note", "c"),
            ("close", "-"),
            ("close", "+")
        ]),
        "a{>>c<<}"
    );
    // A whitespace insertion is real content.
    assert_eq!(
        script(&[
            ("t", "a"),
            ("open", "+"),
            ("t", " "),
            ("close", "+"),
            ("t", "b")
        ]),
        "a{++ ++}b"
    );
}

#[test]
fn comments_inside_a_highlight_follow_it() {
    let steps = [
        ("open", "="),
        ("t", "a"),
        ("note", "one"),
        ("open", "+"),
        ("t", "b"),
        ("note", "two"),
        ("close", "+"),
        ("close", "="),
        ("t", "."),
    ];
    assert_eq!(script(&steps), "{==a{++b++}==}{>>one<<}{>>two<<}.");
    // Two highlights separated by a comment stay separate.
    let steps = [
        ("open", "="),
        ("t", "a"),
        ("close", "="),
        ("note", "x"),
        ("open", "="),
        ("t", "b"),
        ("close", "="),
    ];
    assert_eq!(script(&steps), "{==a==}{>>x<<}{==b==}");
}

#[test]
fn overlapping_highlights_become_their_union() {
    let steps = [
        ("open", "="),
        ("t", "a"),
        ("open", "="),
        ("t", "b"),
        ("close", "="),
        ("note", "A"),
        ("t", "c"),
        ("close", "="),
        ("note", "B"),
    ];
    assert_eq!(script(&steps), "{==abc==}{>>A<<}{>>B<<}");
}

#[test]
fn unclosed_spans_close_at_the_end_and_stray_closes_are_ignored() {
    assert_eq!(
        script(&[("close", "+"), ("open", "-"), ("t", "a")]),
        "{--a--}"
    );
}

#[test]
fn raw_markdown_is_content_whose_delimiters_are_spaced_apart() {
    // A backslash would change the LaTeX; a space does not, and it keeps a
    // `--}` inside an equation from closing the deletion around it.
    assert_eq!(
        script(&[
            ("open", "-"),
            ("raw", "$x_{--}$ {++ ++} ~> {== ==} {>> <<} {~~ ~~}"),
            ("close", "-"),
            ("raw", "")
        ]),
        "{--$x_{ -- }$ { ++ ++ } ~ > { == == } { >> << } { ~~ ~~ }--}"
    );
    assert_eq!(defuse("$a+b$"), "$a+b$");
}

#[test]
fn a_tracked_break_finds_the_real_closer_past_an_equation() {
    let mut critic = Critic::default();
    critic.open(Del, Some("Ana"));
    critic.raw("$x_{--}$");
    critic.close(Del);
    let body = critic.into_inline().render(true);
    let mut out = "{--A--}{>>Ana<<}".to_string();
    splice(
        &mut out,
        "\n\n",
        "",
        &body,
        Some(&(Del, Some("Ana".into()))),
        false,
    );
    assert_eq!(out, "{--A\n\n$x_{ -- }$--}{>>Ana<<}");
}

#[test]
fn formatting_and_links_stay_inside_the_markers() {
    let mut critic = Critic::default();
    critic.open(Ins, None);
    critic.push("bold", true, false, None);
    critic.close(Ins);
    critic.push(" ", false, false, None);
    critic.open(Del, None);
    critic.push("site", false, false, Some("https://x.test"));
    critic.close(Del);
    critic.push("", false, false, None);
    assert_eq!(md(critic), "{++**bold**++} {--[site](https://x.test)--}");
}

#[test]
fn blankness_ignores_markers_but_not_comments() {
    let mut critic = Critic::default();
    critic.open(Ins, None);
    text(&mut critic, "  ");
    critic.raw(" ");
    critic.close(Ins);
    assert!(critic.is_blank());
    critic.comment("note");
    assert!(!critic.is_blank());
    let mut raw = Critic::default();
    raw.raw("![x](y)");
    assert!(!raw.is_blank());
}

#[test]
fn delimiters_in_document_text_are_escaped() {
    assert_eq!(escape("plain"), "plain");
    assert_eq!(escape("a{b}c"), "a{b}c");
    assert_eq!(
        escape("{++ {-- {~~ {>> {== ++} --} ~~} <<} ==} ~>"),
        "{\\++ {\\-- {\\~~ {\\>> {\\== ++\\} --\\} ~~\\} <<\\} ==\\} ~\\>"
    );
    assert_eq!(escape("{+++}"), "{\\+++\\}");
    assert_eq!(
        script(&[("open", "+"), ("t", "a --} b"), ("close", "+")]),
        "{++a --\\} b++}"
    );
}

#[test]
fn splice_marks_a_tracked_break_and_extends_adjacent_spans() {
    let join = |out: &str, body: &str, mark: Option<Mark>| {
        let mut out = out.to_string();
        splice(
            &mut out,
            "\n\n",
            "",
            body,
            mark.map(|m| (m, None)).as_ref(),
            false,
        );
        out
    };
    assert_eq!(join("A", "B", None), "A\n\nB");
    assert_eq!(join("A", "B", Some(Del)), "A{--\n\n--}B");
    assert_eq!(join("A", "B", Some(Ins)), "A{++\n\n++}B");
    assert_eq!(join("{--A--}", "{--B--}", Some(Del)), "{--A\n\nB--}");
    assert_eq!(
        join("Keep {--old--}", "B", Some(Del)),
        "Keep {--old\n\n--}B"
    );
    assert_eq!(join("A", "{++B++} rest", Some(Ins)), "A{++\n\nB++} rest");
    // An escaped delimiter in the text is not a span to extend.
    assert_eq!(join("a --\\}", "b", Some(Del)), "a --\\}{--\n\n--}b");
    let mut heading = "A".to_string();
    splice(&mut heading, "\n\n", "## ", "B", Some(&(Ins, None)), false);
    assert_eq!(heading, "A{++\n\n## ++}B");
}

#[test]
fn join_marked_uses_each_paragraph_mark_and_resets_on_blanks() {
    let bare = (false, false);
    let parts = vec![
        ("a".to_string(), Some((Del, None)), bare),
        ("b".to_string(), None, bare),
        ("c".to_string(), Some((Ins, None)), bare),
        ("  ".to_string(), Some((Del, None)), bare),
        ("d".to_string(), Some((Del, None)), bare),
    ];
    assert_eq!(join_marked(parts, " "), "a{-- --}b c d");
    assert_eq!(join_marked(Vec::new(), " "), "");
}

#[test]
fn a_space_before_a_change_that_ends_the_paragraph_goes_inside_it() {
    let joined = |steps: &[(&str, &str)]| {
        let mut critic = Critic::default();
        critic.set_inline_end();
        for (op, value) in steps {
            match *op {
                "open" => critic.open(kind(value), None),
                "close" => critic.close(kind(value)),
                _ => text(&mut critic, value),
            }
        }
        md(critic)
    };
    assert_eq!(
        joined(&[("t", "in "), ("open", "+"), ("t", "nested"), ("close", "+")]),
        "in{++ nested++}"
    );
    assert_eq!(
        joined(&[("t", "kept "), ("open", "-"), ("t", "gone"), ("close", "-")]),
        "kept{-- gone--}"
    );
    // Not at the end, or not a change: left as it is.
    assert_eq!(
        joined(&[
            ("t", "a "),
            ("open", "+"),
            ("t", "b"),
            ("close", "+"),
            ("t", " c")
        ]),
        "a {++b++} c"
    );
    assert_eq!(
        joined(&[("t", "a "), ("open", "="), ("t", "b"), ("close", "=")]),
        "a {==b==}"
    );
    // A paragraph of its own keeps the space where it was.
    assert_eq!(
        script(&[("t", "in "), ("open", "+"), ("t", "nested"), ("close", "+")]),
        "in {++nested++}"
    );
}

#[test]
fn a_wholly_changed_paragraph_takes_the_break_before_it() {
    let bare = (false, false);
    let parts = vec![
        ("Para".to_string(), None, bare),
        ("{++Added para++}{>>Ana<<}".to_string(), None, bare),
    ];
    assert_eq!(
        join_marked(parts, "<br>"),
        "Para{++<br>Added para++}{>>Ana<<}"
    );
    let parts = vec![
        ("{--Gone--}".to_string(), None, bare),
        ("Kept".to_string(), None, bare),
    ];
    assert_eq!(join_marked(parts, "<br>"), "{--Gone<br>--}Kept");
}

#[test]
fn a_space_trimmed_at_a_tracked_break_comes_back_after_it() {
    // `notice.` + ` The` with the break deleted reads `notice. The` once
    // accepted; the space stays outside the change.
    let parts = vec![
        ("notice.".to_string(), Some((Del, None)), (false, false)),
        ("The".to_string(), None, (true, false)),
        ("before".to_string(), Some((Ins, None)), (false, true)),
        ("accepting".to_string(), None, (false, false)),
    ];
    assert_eq!(
        join_marked(parts, "<br>"),
        "notice.{--<br>--} The<br>before{++<br>++} accepting"
    );
    // An untracked break needs no space: it stays a break.
    let parts = vec![
        ("a".to_string(), None, (false, true)),
        ("b".to_string(), None, (true, false)),
    ];
    assert_eq!(join_marked(parts, "<br>"), "a<br>b");
}

#[test]
fn each_change_is_followed_by_its_author_and_date() {
    let steps = [
        ("by+", "Ana (2026-01-02T03:04:00Z)"),
        ("t", "new"),
        ("close", "+"),
        ("t", " and "),
        ("by-", "Bo"),
        ("t", "old"),
        ("close", "-"),
        ("t", "."),
    ];
    assert_eq!(
        script(&steps),
        "{++new++}{>>Ana (2026-01-02T03:04:00Z)<<} and {--old--}{>>Bo<<}."
    );
}

#[test]
fn a_substitution_names_one_author_or_both() {
    let same = [
        ("by-", "Ana"),
        ("t", "old"),
        ("close", "-"),
        ("by+", "Ana"),
        ("t", "new"),
        ("close", "+"),
    ];
    assert_eq!(script(&same), "{~~old~>new~~}{>>Ana<<}");
    // Insertion first still reads old ~> new, and each author follows in
    // the order of old then new.
    let different = [
        ("by+", "Bo"),
        ("t", "new"),
        ("close", "+"),
        ("by-", "Ana"),
        ("t", "old"),
        ("close", "-"),
    ];
    assert_eq!(script(&different), "{~~old~>new~~}{>>Ana<<}{>>Bo<<}");
    let one_side = [
        ("open", "-"),
        ("t", "old"),
        ("close", "-"),
        ("by+", "Bo"),
        ("t", "new"),
        ("close", "+"),
    ];
    assert_eq!(script(&one_side), "{~~old~>new~~}{>>Bo<<}");
    let other_side = [
        ("by-", "Ana"),
        ("t", "old"),
        ("close", "-"),
        ("open", "+"),
        ("t", "new"),
        ("close", "+"),
    ];
    assert_eq!(script(&other_side), "{~~old~>new~~}{>>Ana<<}");
}

#[test]
fn neighbours_join_only_when_made_by_the_same_person_at_the_same_time() {
    let same = [
        ("by+", "Ana"),
        ("t", "a"),
        ("close", "+"),
        ("by+", "Ana"),
        ("t", "b"),
        ("close", "+"),
    ];
    assert_eq!(script(&same), "{++ab++}{>>Ana<<}");
    let different = [
        ("by+", "Ana"),
        ("t", "a"),
        ("close", "+"),
        ("by+", "Bo"),
        ("t", "b"),
        ("close", "+"),
    ];
    assert_eq!(script(&different), "{++a++}{>>Ana<<}{++b++}{>>Bo<<}");
}

#[test]
fn attributions_stay_with_their_change_inside_a_highlight() {
    // A Word comment moves after the highlight; the attribution does not.
    let steps = [
        ("open", "="),
        ("t", "a"),
        ("by+", "Bo"),
        ("t", "b"),
        ("close", "+"),
        ("note", "Ana: why?"),
        ("close", "="),
    ];
    assert_eq!(script(&steps), "{==a{++b++}{>>Bo<<}==}{>>Ana: why?<<}");
}

#[test]
fn merged_nesting_keeps_the_outer_author_and_split_spans_keep_theirs() {
    let nested = [
        ("by-", "Row"),
        ("t", "a"),
        ("by-", "Run"),
        ("t", "b"),
        ("close", "-"),
        ("close", "-"),
    ];
    assert_eq!(script(&nested), "{--ab--}{>>Row<<}");
    let crossing = [
        ("open", "="),
        ("t", "a"),
        ("by+", "Bo"),
        ("t", "b"),
        ("close", "="),
        ("t", "c"),
        ("close", "+"),
    ];
    assert_eq!(script(&crossing), "{==a{++b++}{>>Bo<<}==}{++c++}{>>Bo<<}");
    // An empty change leaves no attribution behind.
    assert_eq!(script(&[("t", "x"), ("by+", "Bo"), ("close", "+")]), "x");
}

#[test]
fn a_tracked_break_extends_spans_only_by_the_same_person() {
    let join = |out: &str, body: &str, change: Option<Change>| {
        let mut out = out.to_string();
        splice(&mut out, "\n\n", "", body, change.as_ref(), false);
        out
    };
    let ana = || Some((Del, Some("Ana".to_string())));
    assert_eq!(join("A", "B", ana()), "A{--\n\n--}{>>Ana<<}B");
    assert_eq!(
        join("{--A--}{>>Ana<<}", "{--B--}{>>Ana<<} rest", ana()),
        "{--A\n\nB--}{>>Ana<<} rest"
    );
    // Someone else's deletion on either side stays its own span.
    assert_eq!(
        join("{--A--}{>>Bo<<}", "{--B--}{>>Bo<<}", ana()),
        "{--A--}{>>Bo<<}{--\n\n--}{>>Ana<<}{--B--}{>>Bo<<}"
    );
    // A body span of the same kind whose closer is followed by nothing
    // attributed is not the same change.
    assert_eq!(
        join("A", "{--B--} rest", ana()),
        "A{--\n\n--}{>>Ana<<}{--B--} rest"
    );
    assert_eq!(join("A", "{--B", ana()), "A{--\n\n--}{>>Ana<<}{--B");
}

#[test]
fn tree_ignores_a_close_with_nothing_open() {
    assert_eq!(tree(vec![Token::Close(Ins)]), Vec::<Node>::new());
}

#[test]
fn a_separator_next_to_a_wholly_changed_part_joins_its_change() {
    let by = "{>>Ana<<}";
    let joined =
        |parts: &[&str]| join_changed(parts.iter().map(|part| part.to_string()).collect(), "; ");
    assert_eq!(
        joined(&["in {++nested++}", &format!("{{--gone--}}{by}")]),
        format!("in {{++nested++}}{{--; gone--}}{by}")
    );
    assert_eq!(
        joined(&[&format!("{{--gone--}}{by}"), "kept", "{++new++}"]),
        format!("{{--gone; --}}{by}kept{{++; new++}}")
    );
    // Text around the change keeps the separator outside it.
    assert_eq!(
        joined(&["a {--b--}", "c {++d++} e"]),
        "a {--b--}; c {++d++} e"
    );
}

#[test]
fn a_change_inside_bold_text_keeps_one_bold_run() {
    let mut critic = Critic::default();
    critic.push("around ", true, false, None);
    critic.open(Ins, Some("Ana"));
    critic.push("an insertion", true, false, None);
    critic.close(Ins);
    critic.push(" inside", true, false, None);
    critic.push(" end", false, false, None);
    assert_eq!(
        md(critic),
        "**around {++an insertion++}{>>Ana<<} inside** end"
    );
    // Bold text next to a plain change stays apart from it.
    let mut critic = Critic::default();
    critic.push("bold", true, false, None);
    critic.open(Del, None);
    critic.push(" plain", false, false, None);
    critic.close(Del);
    assert_eq!(md(critic), "**bold**{-- plain--}");
}

#[test]
fn a_change_inside_link_text_keeps_one_link() {
    let mut critic = Critic::default();
    let url = Some("https://example.com");
    critic.push("the ", false, false, url);
    critic.open(Ins, Some("Ana"));
    critic.push("new ", false, false, url);
    critic.close(Ins);
    critic.push("page", false, false, url);
    assert_eq!(
        md(critic),
        "[the {++new ++}{>>Ana<<}page](https://example.com)"
    );
    // A change around a whole link, or between two different links,
    // stays outside them.
    let mut critic = Critic::default();
    critic.push("see ", false, false, None);
    critic.open(Del, None);
    critic.push("old", false, false, Some("https://a.example"));
    critic.close(Del);
    critic.push(" and ", false, false, Some("https://b.example"));
    assert_eq!(
        md(critic),
        "see {--[old](https://a.example)--} [and](https://b.example)"
    );
}

#[test]
fn a_delimiter_split_across_runs_is_escaped() {
    // Word splits runs anywhere; `a -` + `-} b` must not close the deletion.
    assert_eq!(
        script(&[("open", "-"), ("t", "a -"), ("t", "-} b"), ("close", "-")]),
        "{--a --\\} b--}"
    );
    // An empty span between the two pieces is dropped, so they still meet.
    assert_eq!(
        script(&[
            ("open", "-"),
            ("t", "x {"),
            ("open", "+"),
            ("close", "+"),
            ("t", "++ y"),
            ("close", "-")
        ]),
        "{--x {\\++ y--}"
    );
}
