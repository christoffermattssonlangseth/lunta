//! Fonts that draw symbols under the names of letters (the Rust side of `PDFSymbolFonts.swift`).
//!
//! Elsevier's and Springer's typesetters (Advent) set bullets, ©, Greek and math with "Pi" and
//! symbol fonts whose glyphs are named like letters, mostly with no ToUnicode map (or one that just
//! restates the names): Cell's Highlights draw their bullets as "d" in AdvPSMPi6 and
//! "© 2024 The Author(s)" as "ª" in AdvPSSym, so the text read "d ISS elucidates…" and "ª 2024".
//! The tables were drawn from the glyphs of the fonts embedded in the evaluation's PDFs and the test
//! library (Symbol's and TeX's OMS layout where a font follows one). PDFium gives such a glyph the
//! character of its name ("d", "ª"), so here they go by that character; a glyph whose name has no
//! character ("C211", "H11001") PDFium gives its code, which says nothing without the font's
//! Differences, and is left as it is. Advent's cmsy (AdvP4C4E74) often has a right ToUnicode map
//! that gives "(" or "=", so only its Latin-1 stand-ins ("ð" for "(") are mapped.

/// (font name endings, [(what PDFium reads, what the glyph is)])
static FAMILIES: &[(&[&str], &[(&str, &str)])] = &[
    // Advent's Symbol (AdvPSSym, AdvP697C): Symbol's layout under StandardEncoding's names ("a" α, "ordfeminine" ©) and "Cnnn" (Symbol's code nnn: "C211" ©)
    (&["AdvPSSym", "AdvP697C"], &[("\"", "∀"), ("$", "∃"), ("'", "♥"), ("*", "∗"), ("-", "−"), ("@", "≅"), ("A", "Α"), ("B", "Β"), ("C", "Χ"), ("D", "Δ"), ("E", "Ε"), ("F", "Φ"), ("G", "Γ"), ("H", "Η"), ("I", "Ι"), ("J", "ϑ"), ("K", "Κ"), ("L", "Λ"), ("M", "Μ"), ("N", "Ν"), ("O", "Ο"), ("P", "Π"), ("Q", "Θ"), ("R", "Ρ"), ("S", "Σ"), ("T", "Τ"), ("U", "Υ"), ("V", "ς"), ("W", "Ω"), ("X", "Ξ"), ("Y", "Ψ"), ("Z", "Ζ"), ("\\", "∴"), ("^", "⊥"), ("`", "ℑ"), ("a", "α"), ("b", "β"), ("c", "χ"), ("d", "δ"), ("e", "ε"), ("f", "φ"), ("g", "γ"), ("h", "η"), ("i", "ι"), ("j", "ϕ"), ("k", "κ"), ("l", "λ"), ("m", "μ"), ("n", "ν"), ("o", "ο"), ("p", "π"), ("q", "θ"), ("r", "ρ"), ("s", "σ"), ("t", "τ"), ("u", "υ"), ("v", "ϖ"), ("w", "ω"), ("x", "ξ"), ("y", "ψ"), ("z", "ζ"), ("~", "∼"), ("¡", "ϒ"), ("¢", "′"), ("£", "≤"), ("¤", "♦"), ("¥", "∞"), ("§", "♣"), ("¨", "∪"), ("ª", "©"), ("«", "↔"), ("¯", "⊕"), ("´", "ℜ"), ("¶", "∂"), ("·", "×"), ("¸", "⊄"), ("º", "⎣"), ("»", "≈"), ("¿", "↵"), ("Å", "⎜"), ("Æ", "〈"), ("Ç", "⎛"), ("Ø", "⎡"), ("ß", "⎦"), ("å", "⎟"), ("æ", "〉"), ("ç", "⎞"), ("ø", "⎤"), ("ı", "⌡"), ("Ł", "⎝"), ("ł", "⎠"), ("Œ", "⎢"), ("œ", "⎥"), ("ˆ", "℘"), ("ˇ", "∉"), ("˘", "∅"), ("˙", "∩"), ("˚", "⊇"), ("˛", "∈"), ("˜", "⊗"), ("˝", "⊆"), ("–", "±"), ("—", "∠"), ("’", "∋"), ("‚", "÷"), ("“", "♠"), ("”", "≡"), ("„", "≠"), ("†", "″"), ("‡", "≥"), ("‰", "⏐"), ("‹", "←"), ("›", "↑")]),
    // Advent's cmsy (AdvP4C4E74): TeX's OMS layout ("exclam" →, "C2" ×), and ( ) + = [ ] as "eth" "Thorn" "thorn" "onequarter" "onehalf" "C138"
    (&["AdvP4C4E74"], &[("¼", "="), ("½", "["), ("Ð", "⇌"), ("Þ", ")"), ("ð", "("), ("þ", "+"), ("‘", "⊢"), ("’", "≃"), ("€", "♠")]),
    // Springer's and Elsevier's TeX CM Maths Symbols: ( ) + = as "eth" "Thorn" "thorn" "onequarter"
    (&["TeXCMMathsSymbols", "TeX_CM_Maths_Symbols"], &[("¼", "="), ("Þ", ")"), ("ð", "("), ("þ", "+")]),
    // Advent's Math Pi 6: the bullet of Cell's Highlights as "d"
    (&["AdvPSMPi6"], &[("d", "•")]),
    // Advent's Pi 1: Greek and operators ("six" ×, "period" •)
    (&["AdvPi1"], &[("!", "∝"), ("\"", "∮"), ("#", "∏"), ("$", "∫"), ("%", "≅"), ("&", "≈"), ("(", "⊂"), (")", "⊃"), ("*", "∼"), ("+", "±"), (",", "∵"), (".", "•"), ("/", "∓"), ("1", "∅"), ("4", ">"), ("5", "<"), ("6", "×"), ("7", "−"), ("8", "°"), (":", "≡"), (";", "∴"), ("<", "≪"), (">", "≫"), ("?", "∞"), ("@", "″"), ("B", "ς"), ("C", "Ψ"), ("D", "Δ"), ("E", "ϵ"), ("F", "Φ"), ("G", "Γ"), ("H", "∇"), ("K", "ϰ"), ("L", "Λ"), ("O", "Ω"), ("P", "Π"), ("R", "ϱ"), ("S", "Σ"), ("U", "ϒ"), ("V", "∨"), ("W", "ϑ"), ("X", "Ξ"), ("Y", "Θ"), ("Z", "η"), ("[", "⟦"), ("\\", "⊥"), ("]", "⟧"), ("^", "≃"), ("_", "…"), ("a", "α"), ("b", "β"), ("c", "ψ"), ("d", "δ"), ("e", "ε"), ("f", "ϕ"), ("g", "γ"), ("h", "ℏ"), ("i", "ι"), ("j", "φ"), ("k", "κ"), ("l", "λ"), ("m", "μ"), ("n", "ν"), ("o", "ω"), ("p", "π"), ("q", "∂"), ("r", "ρ"), ("s", "σ"), ("t", "τ"), ("u", "υ"), ("w", "χ"), ("x", "ξ"), ("y", "θ"), ("z", "ζ"), ("{", "†"), ("}", "§"), ("~", "∑"), ("’", "′")]),
    // Advent's Pi 2: open legend symbols and Greek
    (&["AdvPi2"], &[("&", "□"), ("*", "○"), ("k", "κ"), ("m", "μ"), ("~", "△")]),
    // Advent's Pi 3: filled legend symbols, ≠, © (Springer's copyright line) and ®
    (&["AdvPi3"], &[("#", "©"), ("&", "■"), ("*", "●"), ("1", "®"), ("=", "≠"), ("~", "▲")]),
    // Advent's Times symbols: the glyphs not where their names say
    (&["AdvTir_symb"], &[("-", "−"), ("9", "×"), (":", "±"), (";", "∓"), ("?", "+"), ("A", "∼"), ("B", "≤"), ("C", "≥"), ("þ", "±")]),
    // Advent's Greek (AdvPSMP13): Greek by Latin letter ("l" μ)
    (&["AdvPSMP13"], &[("!", "ϒ"), ("$", "∇"), ("(", "∏"), (")", "∑"), ("*", "∏"), ("+", "∑"), (",", "ϰ"), ("-", "ϖ"), (".", "ϱ"), ("/", "ϕ"), ("0", "ϑ"), ("1", "ς"), ("A", "Α"), ("B", "Β"), ("C", "Γ"), ("D", "Δ"), ("E", "Ε"), ("F", "Ζ"), ("G", "Η"), ("H", "Θ"), ("I", "Ι"), ("J", "Κ"), ("K", "Λ"), ("L", "Μ"), ("M", "Ν"), ("N", "Ξ"), ("O", "Ο"), ("P", "Π"), ("Q", "Ρ"), ("R", "Σ"), ("S", "Τ"), ("T", "Υ"), ("U", "Φ"), ("V", "Χ"), ("W", "Ψ"), ("X", "Ω"), ("a", "α"), ("b", "β"), ("c", "γ"), ("d", "δ"), ("e", "ε"), ("f", "ζ"), ("g", "η"), ("h", "θ"), ("i", "ι"), ("j", "κ"), ("k", "λ"), ("l", "μ"), ("m", "ν"), ("n", "ξ"), ("o", "ο"), ("p", "π"), ("q", "ρ"), ("r", "σ"), ("s", "τ"), ("t", "υ"), ("u", "φ"), ("v", "χ"), ("w", "ψ"), ("x", "ω")]),
    // Advent's math italic Greek (AdvPSMP4): Greek and operators by Latin names (its accents are left alone)
    (&["AdvPSMP4"], &[("!", "Γ"), ("\"", "Λ"), ("#", "Θ"), ("$", "Ξ"), ("%", "Π"), ("&", "Σ"), ("(", "Ψ"), (")", "Ω"), ("*", "Δ"), ("+", "∇"), (":", "∫"), (";", "∑"), ("<", "∏"), ("@", "′"), ("A", "″"), ("B", "≈"), ("D", "|"), ("E", "‖"), ("K", "∴"), ("L", "∂"), ("M", "{"), ("N", "}"), ("O", "≪"), ("P", "≫"), ("S", "⟨"), ("T", "⟩"), ("U", "⊆"), ("V", "⊇"), ("W", "∩"), ("X", "∪"), ("[", ">"), ("\\", "<"), ("]", "≥"), ("^", "≤"), ("a", "α"), ("b", "β"), ("c", "γ"), ("d", "δ"), ("e", "ε"), ("f", "ζ"), ("g", "η"), ("h", "θ"), ("i", "κ"), ("j", "λ"), ("k", "μ"), ("l", "ν"), ("m", "ξ"), ("n", "π"), ("o", "ρ"), ("p", "σ"), ("q", "τ"), ("r", "φ"), ("s", "χ"), ("t", "ψ"), ("u", "ω"), ("’", "Φ")]),
    // Advent's Greek (AdvPSMP10)
    (&["AdvPSMP10"], &[("/", "ϕ"), ("a", "α"), ("b", "β"), ("u", "φ"), ("v", "χ")]),
    // Advent's Greek (AdvPS3F4C13)
    (&["AdvPS3F4C13"], &[("a", "α"), ("b", "β"), ("d", "δ"), ("g", "γ"), ("k", "κ"), ("m", "μ")]),
    // Copyright signs in odd places ("f", "ampersand", "C223")
    (&["AdvEURO4", "AdvTA42", "AdvT377"], &[("&", "©"), ("f", "©")]),
    // Elsevier entities 3: legend shapes
    (&["AdvEls-ent3"], &[("E", "◇"), ("J", "○"), ("K", "●")]),
    // Elsevier entities 5
    (&["AdvEls-ent5"], &[("1", "°")]),
    // Elsevier entities 8
    (&["AdvEls-ent8"], &[("7", "±")]),
    // Advent's ×
    (&["AdvP7DED"], &[("3", "×")]),
    // Springer's envelope (corresponding author)
    (&["AdvTT4ff65459"], &[("*", "✉")]),
];

/// The table of the font named `name` (with or without its subset prefix: "HBKIKE+AdvPSMPi6";
/// Springer's "XcyghdAdvPi3" ends with AdvPi3; "Wingdings-Regular" is Wingdings).
pub fn family(name: &str) -> Option<&'static [(&'static str, &'static str)]> {
    let name = name.split_once('+').map(|x| x.1).unwrap_or(name);
    for (keys, table) in FAMILIES {
        for k in *keys {
            if name.ends_with(k) {
                return Some(table);
            }
            if let Some(rest) = name.strip_prefix(k) {
                if !k.ends_with(|c: char| c.is_ascii_digit()) && (rest == "-Regular" || rest.chars().all(|c| c.is_ascii_digit())) {
                    return Some(table);
                }
            }
        }
    }
    None
}

/// What a glyph of a symbol font is, from the character PDFium read for it.
pub fn map(table: &[(&str, &'static str)], ch: char) -> Option<&'static str> {
    let mut buf = [0u8; 4];
    let s = ch.encode_utf8(&mut buf);
    table.iter().find(|(k, _)| *k == s).map(|(_, v)| *v)
}
