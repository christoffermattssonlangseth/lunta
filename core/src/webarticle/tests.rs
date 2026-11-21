use super::*;

const JEKYLL: &str = include_str!("../testdata/web/jekyll-blog.html");
const SUBSTACK: &str = include_str!("../testdata/web/substack-post.html");
const NEWS: &str = include_str!("../testdata/web/news-article.html");
const DOCS: &str = include_str!("../testdata/web/docs-page.html");

fn all(a: &WebArticle) -> String {
    a.chapters.iter().map(|c| c.html.as_str()).collect()
}

#[test]
fn jekyll_post_with_code() {
    let a = extract(JEKYLL, "https://karpathy.github.io/2019/04/25/recipe/", "A Recipe for Training Neural Networks");
    assert!(a.problem.is_empty(), "{}", a.problem);
    let h = all(&a);
    // the title once (the page's own h1 is left out), then a chapter at the post's top level (h2)
    let titles: Vec<&str> = a.chapters.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["A Recipe for Training Neural Networks", "The recipe"]);
    assert_eq!(h.matches("A Recipe for Training Neural Networks").count(), 1, "{h}");
    // the h4s are the chapters' sections
    assert!(h.contains("<h2 id=\"x1-neural-net-training-is-a-leaky-abstraction\">1) Neural net training is a leaky abstraction</h2>"), "{h}");
    assert_eq!(a.chapters[1].headings.iter().map(|x| x.title.as_str()).collect::<Vec<_>>(), vec!["The recipe", "1. Become one with the data", "Conclusion"]);
    // code: monospaced, its language, its lines and indentation as they were
    assert!(h.contains("<pre><code class=\"language-python\">&gt;&gt;&gt; data = load()\n&gt;&gt;&gt; model = Net(data)\n# indentation must survive\n    model.fit()</code></pre>"), "{h}");
    assert!(h.contains("<code>-log(1/n_classes)</code>"));
    assert!(h.contains("<blockquote><p>A quoted line that has to stay a quote.</p></blockquote>"), "{h}");
    assert!(h.contains("<li><strong>fix random seed</strong>. Always use"), "{h}");
    assert!(h.contains("<a href=\"https://twitter.com/example/status/1\">tweet</a>"));
    // the image, absolute, to be fetched (the 40-pixel feed icon isn't)
    assert_eq!(a.images, vec!["https://karpathy.github.io/assets/recipe/loss.png"]);
    assert!(h.contains("<img src=\"lunta-image:0\" alt=\"a loss curve\"/>"));
    // no navigation, date line, comments or footer
    for gone in ["About", "Apr 25, 2019", "Disqus", "disqus", "Musings", "enable JavaScript", "GoogleAnalytics"] {
        assert!(!h.contains(gone), "{gone} in {h}");
    }
    assert!(a.words > 150, "{}", a.words);
}

#[test]
fn images_go_in_or_are_left_out() {
    let a = extract(JEKYLL, "https://karpathy.github.io/2019/04/25/recipe/", "A Recipe for Training Neural Networks");
    let h = all(&a);
    let with = with_images(&h, &[Some("web1.png".into())]);
    assert!(with.contains("<img src=\"../images/web1.png\" alt=\"a loss curve\"/>"));
    let without = with_images(&h, &[None]);
    assert!(!without.contains("<img"));
    assert!(!without.contains("<p></p>"));
}

#[test]
fn substack_post_without_widgets() {
    let a = extract(SUBSTACK, "https://gradientnotes.substack.com/p/why-small-models-win", "Why Small Models Win");
    assert!(a.problem.is_empty());
    let h = all(&a);
    for gone in ["Subscribe to get", "A long comment", "Sign in", "Archive", "Maria Lindqvist", "Like"] {
        assert!(!h.contains(gone), "{gone} in {h}");
    }
    assert!(h.contains("<h1 id=\"the-cost-of-size\">The cost of size</h1>"), "{h}");
    assert!(h.contains("<ol><li>Start with the smallest model that could work.</li><li>Measure before you scale.</li></ol>"), "{h}");
    // the largest of the srcset
    assert_eq!(a.images, vec!["https://substackcdn.com/image/fetch/w_1456/chart.png"]);
    assert!(h.contains("<figure><img src=\"lunta-image:0\" alt=\"cost chart\"/><figcaption>Cost grows faster than quality.</figcaption></figure>"), "{h}");
}

#[test]
fn news_article_with_table() {
    let a = extract(NEWS, "https://www.example-times.com/science/2023/city-library", "City Opens New Science Library");
    let h = all(&a);
    assert_eq!(a.chapters.len(), 1);
    assert!(h.contains("<table><caption>Opening hours</caption><tr><th>Day</th><th>Hours</th></tr><tr><td>Weekdays</td><td>8–24</td></tr>"), "{h}");
    for gone in ["We use cookies", "Another story", "Facebook", "All rights reserved", "Home", "By Priya"] {
        assert!(!h.contains(gone), "{gone} in {h}");
    }
    assert!(h.contains("harbour front on Thursday"));
}

#[test]
fn docs_page_math_and_pre() {
    let a = extract(DOCS, "https://tool.example.org/docs/config", "Configuration");
    let h = all(&a);
    assert!(h.contains("<pre><code>[build]\njobs = 4</code></pre>"), "{h}");
    assert!(h.contains("<math xmlns=\"http://www.w3.org/1998/Math/MathML\"><semantics><mrow><mi>x</mi><mo>=</mo><mn>2</mn></mrow><annotation encoding=\"application/x-tex\">x=2</annotation></semantics></math>"), "{h}");
    // KaTeX's drawing isn't there twice
    assert_eq!(h.matches("x=2").count(), 1, "{h}");
    assert!(!h.contains("FAQ"));
}

#[test]
fn too_little_text() {
    let a = extract("<html><body><div id=app></div><p>Loading…</p></body></html>", "https://app.example.com/", "App");
    assert!(!a.problem.is_empty());
}

#[test]
fn urls() {
    let b = "https://example.com/blog/2020/post/";
    assert_eq!(join_url(b, "img/a.png"), "https://example.com/blog/2020/post/img/a.png");
    assert_eq!(join_url(b, "../a.png"), "https://example.com/blog/2020/a.png");
    assert_eq!(join_url(b, "/a.png"), "https://example.com/a.png");
    assert_eq!(join_url(b, "//cdn.example.com/a.png"), "https://cdn.example.com/a.png");
    assert_eq!(join_url(b, "https://x.org/y"), "https://x.org/y");
    assert_eq!(join_url("https://example.com", "a.png"), "https://example.com/a.png");
    assert_eq!(join_url("https://example.com/a/b.html?x=1", "c.png?v=2"), "https://example.com/a/c.png?v=2");
}
