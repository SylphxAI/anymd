use crate::Options;

use super::convert;

fn md(html: &str) -> String {
    convert(html.as_bytes(), &Options::default())
        .unwrap()
        .sections[0]
        .markdown
        .clone()
}

#[test]
fn extracts_article_and_drops_chrome() {
    let html = r#"<!doctype html><html><head><title>My Post</title>
            <meta name="description" content="A short post.">
            <script>var x = 1;</script><style>p{}</style></head>
            <body><header><a href="/">Logo</a><nav><a href="/a">Home</a></nav></header>
            <div class="cookie-banner">We use cookies. <button>OK</button></div>
            <main><article><h1>Hello   World</h1>
            <p>This is <strong>bold</strong> and <em>soft</em> text with a <a href="/docs/x">relative link</a>.
            Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore
            et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.</p>
            <ul><li>one</li><li>two<ul><li>nested</li></ul></li></ul>
            <ol start="3"><li>three</li><li>four</li></ol>
            <pre><code class="language-rust">fn main() {
    println!("hi");
}</code></pre>
            <img src="/a.png"><img src="b.png" alt="A chart">
            </article><aside>Related stuff</aside></main>
            <footer>Copyright</footer></body></html>"#;
    let converted = convert(
        html.as_bytes(),
        &Options {
            base_url: Some("https://example.com/blog/post".into()),
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(converted.title.as_deref(), Some("My Post"));
    assert_eq!(
        converted.metadata,
        vec![("description".into(), "A short post.".into())]
    );
    let md = &converted.sections[0].markdown;
    assert!(md.starts_with("# Hello World\n\nThis is **bold** and *soft* text with a [relative link](https://example.com/docs/x)."), "{md}");
    assert!(
        md.contains("- one\n- two\n  - nested\n\n3. three\n4. four"),
        "{md}"
    );
    assert!(
        md.contains("```rust\nfn main() {\n    println!(\"hi\");\n}\n```"),
        "{md}"
    );
    assert!(
        md.contains("![A chart](https://example.com/blog/b.png)"),
        "{md}"
    );
    for noise in [
        "Logo",
        "Home",
        "cookies",
        "Related",
        "Copyright",
        "var x",
        "a.png",
    ] {
        assert!(!md.contains(noise), "{noise} leaked: {md}");
    }
    assert!(!md.contains("\n\n\n"));
}

#[test]
fn web_html_still_drops_navigation_without_a_main_wrapper() {
    let markdown = md("<body><nav epub:type='toc'><h2>Contents</h2><ol><li>Index</li></ol></nav><p>Body text.</p></body>");
    assert_eq!(markdown, "Body text.");
}

#[test]
fn tables_become_pipe_tables() {
    let md = md(
        "<table><caption>Scores</caption><thead><tr><th>Name</th><th>Score</th></tr></thead>\
         <tbody><tr><td>Ann <b>A</b></td><td>1|2</td></tr><tr><td colspan=2>total</td></tr></tbody></table>",
    );
    assert_eq!(
        md,
        "Scores\n\n|Name|Score|\n|-|-|\n|Ann **A**|1\\|2|\n|total||"
    );
}

#[test]
fn layout_tables_render_as_blocks() {
    let md = md("<table><tr><td><p>Only column</p></td></tr><tr><td>Second</td></tr></table>");
    assert_eq!(md, "Only column\n\nSecond");
}

#[test]
fn inline_spacing_and_breaks() {
    assert_eq!(
        md("<p>a<b> b </b>c<br>d <code>x`y</code></p>"),
        "a **b** c\nd ``x`y``"
    );
    assert_eq!(
        md("<p><a href='#top'>Top</a> <a href='javascript:void(0)'>JS</a></p>"),
        "Top JS"
    );
    assert_eq!(
        md("<blockquote><p>q1</p><p>q2</p></blockquote>"),
        "> q1\n>\n> q2"
    );
}

#[test]
fn hidden_and_noise_classes_are_dropped() {
    let body = "Real content paragraph that is long enough to dominate the page text. ".repeat(5);
    let html = format!(
        "<div class='sidebar'>Side</div><div style='display: none'>Hidden</div>\
         <div aria-hidden='true'>Aria</div><div role='navigation'>Menu</div><p>{body}</p>"
    );
    let md = md(&html);
    assert!(md.starts_with("Real content"), "{md}");
    assert!(
        !md.contains("Side")
            && !md.contains("Hidden")
            && !md.contains("Aria")
            && !md.contains("Menu")
    );
}

#[test]
fn xhtml_self_closing_anchors_do_not_swallow_content() {
    let md = md(
        "<?xml version='1.0'?><html><body><h2><a id=\"c1\"/>One<br/>Two</h2><p>A</p><p>B<span class='x'/></p></body></html>",
    );
    assert_eq!(md, "## One Two\n\nA\n\nB");
}

#[test]
fn malformed_input_never_panics() {
    for input in [
        "",
        "<",
        "<<<>>>",
        "<table><tr><td colspan=999999>x",
        "<pre>",
        "\u{0}\u{ffff}",
        "<a href>",
        "<a/",
        "<a b='/>",
        "<é/>",
    ] {
        let _ = md(input);
    }
    let deep = "<div>".repeat(5000) + "deep" + &"</div>".repeat(5000);
    assert!(md(&deep).contains("deep"));
    let _ = convert(&[0xff, 0xfe, 0x00, 0x3c], &Options::default()).unwrap();
}
