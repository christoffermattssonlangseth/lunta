//! Resolve Citations in a .tex file, on Windows and Linux as on the Mac (apple/Mac/TexResolve.swift):
//! the placeholders written while writing (`\cite{?langseth2025}`, `\cite{langseth2025?}`,
//! "(Langseth et al. 2025)", "[Langseth 2025 spatial]"; core/src/placeholders) with Lunta's
//! candidates, each confirmed or skipped by the writer in the window (src/texresolve.ts); then the
//! confirmed papers go into the chosen folder (outside papers added to the library first), whose
//! .bib the app keeps updated, and the .tex is written back with their citation keys, the original
//! kept as <file>.tex.bak. Nothing is written before the writer clicks Write.

use crate::{citeapi, err, AppState, R};
use lunta_core::{placeholders, Library};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use tauri::State;

/// The placeholders in the file.
#[tauri::command]
pub async fn tex_open(path: String) -> R<Value> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Couldn’t read {path} ({e}). Lunta reads .tex files written in UTF-8."))?;
    let found = placeholders::find_tex_placeholders(text);
    Ok(json!({"placeholders": found.iter().map(citeapi::placeholder_json).collect::<Vec<_>>()}))
}

/// Lunta's candidates for a placeholder's part ({cite}) or for the writer's search ({query}),
/// as the citation API's /cite/suggest answers them.
#[tauri::command]
pub async fn tex_suggest(state: State<'_, AppState>, cite: Option<Map<String, Value>>, query: Option<String>) -> R<Value> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || {
        let s = match (cite, query) {
            (Some(c), _) => lib.suggest_citation(citeapi::cite_of(&c), true),
            (None, Some(q)) => lib.search_citation(q, true),
            _ => Default::default(),
        };
        json!({"candidates": s.candidates.iter().map(citeapi::candidate_json).collect::<Vec<_>>(), "exact": s.exact, "note": s.note})
    })
    .await
    .map_err(err)
}

/// What the writer chose for one placeholder: one pick per part (a candidate as tex_suggest gave
/// it, or null to leave the part as written).
#[derive(Deserialize)]
pub struct Choice {
    start: u32,
    end: u32,
    picks: Vec<Option<Map<String, Value>>>,
}

/// Adds the outside papers, puts every confirmed paper in `folder`, writes the .tex with their
/// keys (and the .bak). Returns {written, added, papers, backup}.
#[tauri::command]
pub async fn tex_write(state: State<'_, AppState>, path: String, folder: Option<String>, choices: Vec<Choice>) -> R<Value> {
    let lib = state.lib()?;
    tauri::async_runtime::spawn_blocking(move || write(&lib, &path, folder.as_deref().filter(|f| !f.is_empty()), &choices)).await.map_err(err)?
}

pub(crate) fn write(lib: &Library, path: &str, folder: Option<&str>, choices: &[Choice]) -> R<Value> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Couldn’t read {path} ({e})."))?;
    let mut out = vec![];
    let (mut added, mut all) = (HashSet::new(), HashSet::new());
    let mut known: HashMap<String, String> = HashMap::new(); // an outside paper added once
    for c in choices {
        let mut keys = vec![];
        for pick in &c.picks {
            let Some(p) = pick else {
                keys.push(String::new());
                continue;
            };
            let mut id = p.get("id").and_then(Value::as_str).unwrap_or("").to_string();
            if id.is_empty() {
                let Some(m) = p.get("meta").and_then(Value::as_object).and_then(citeapi::meta) else {
                    return Err("A paper without details can’t be added.".into());
                };
                let k = format!("{}|{}", m.doi.to_lowercase(), m.title);
                if let Some(x) = known.get(&k) {
                    id = x.clone();
                } else {
                    let had = lib.find_cited(m.doi.clone(), m.title.clone()).is_some();
                    let paper = lib.add_found_paper(m).map_err(|e| format!("Couldn’t add the paper ({e}). Nothing was written to the file."))?;
                    if !had {
                        added.insert(paper.id.clone());
                    }
                    known.insert(k, paper.id.clone());
                    id = paper.id;
                }
            }
            if let Some(f) = folder {
                lib.set_membership(f.to_string(), id.clone(), true).map_err(err)?;
            }
            keys.push(lib.cite_keys(vec![id.clone()]).pop().unwrap_or_default());
            all.insert(id);
        }
        out.push(placeholders::PlaceholderChoice { start: c.start, end: c.end, keys });
    }
    let new = placeholders::rewrite_tex_placeholders(text.clone(), out);
    let backup = format!("{path}.bak");
    if new != text {
        std::fs::copy(path, &backup).map_err(|e| format!("Couldn’t keep a copy of the file as {backup} ({e}); nothing was written."))?;
        crate::bibfiles::write_if_changed(std::path::Path::new(path), &new).map_err(|e| format!("Couldn’t write {path} ({e})."))?;
    }
    let written = choices.iter().filter(|c| c.picks.iter().any(Option::is_some)).count();
    Ok(json!({"written": written, "added": added.into_iter().collect::<Vec<_>>(), "papers": all.len(), "backup": backup, "changed": new != text}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunta_core::model::PaperMeta;

    #[test]
    fn writes_the_keys_and_a_backup() {
        let dir = std::env::temp_dir().join(format!("lunta-tex-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let lib = Library::open(dir.join("lib").display().to_string(), dir.join("cache").display().to_string(), "test".into()).unwrap();
        let p = lib
            .add_paper(PaperMeta { title: "Alpha cells".into(), authors: vec!["Ada Lee".into()], date: "2020".into(), ..Default::default() }, None, None, None)
            .unwrap();
        let folder = lib.create_collection("Thesis".into(), None).unwrap();
        let tex = dir.join("main.tex");
        let src = "Cells \\cite{?lee2020} and (Nobody 2018) and (Ng 2021).\n";
        std::fs::write(&tex, src).unwrap();
        let found: Value = tauri::async_runtime::block_on(tex_open(tex.display().to_string())).unwrap();
        let ph = found["placeholders"].as_array().unwrap();
        assert_eq!(ph.len(), 3);
        let at = |i: usize| (ph[i]["start"].as_u64().unwrap() as u32, ph[i]["end"].as_u64().unwrap() as u32);
        let mine = json!({"id": p.id}).as_object().cloned();
        let outside = json!({"id": "", "meta": {"title": "Beta waves", "authors": ["Kim Ng"], "year": "2021"}}).as_object().cloned();
        let choices = vec![
            Choice { start: at(0).0, end: at(0).1, picks: vec![mine] },
            Choice { start: at(1).0, end: at(1).1, picks: vec![None] },
            Choice { start: at(2).0, end: at(2).1, picks: vec![outside] },
        ];
        let r = write(&lib, &tex.display().to_string(), Some(&folder.id), &choices).unwrap();
        assert_eq!((r["written"].clone(), r["papers"].clone(), r["added"].as_array().unwrap().len()), (json!(2), json!(2), 1));
        let out = std::fs::read_to_string(&tex).unwrap();
        assert_eq!(out, "Cells \\cite{Lee2020alpha} and (Nobody 2018) and \\cite{Ng2021beta}.\n");
        assert_eq!(std::fs::read_to_string(dir.join("main.tex.bak")).unwrap(), src);
        assert_eq!(lib.papers(lunta_core::model::PaperFilter { collection: Some(folder.id.clone()), ..Default::default() }, lunta_core::model::SortKey::Added).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
