//! Across science against the live OpenAlex, in a temporary library (nothing of yours is read or
//! written): cargo run --example across_live -- ["interests paragraph"] [per field 1|2]
//! Prints the request count, the picks per field, the fields without one, and where the day's
//! OpenAlex answer was kept (across-science-pool.json: what the tests' recordings are cut from).
use lunta_core::foryou::across::{self, AcrossScience};
use lunta_core::foryou::state::Files;
use lunta_core::*;

fn main() {
    let mut args = std::env::args().skip(1);
    let interests = args.next().unwrap_or_default();
    let per_field: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let tmp = std::env::temp_dir().join(format!("lunta-across-live-{}", std::process::id()));
    let lib_dir = tmp.join("lib");
    std::fs::create_dir_all(&lib_dir).unwrap();
    let lib = Library::open(lib_dir.to_string_lossy().into(), tmp.join("cache").to_string_lossy().into(), "live".into()).unwrap();
    let files = Files::new(&lib_dir, tmp.join("local"));
    files.change(|s| s.interests = interests.clone()).unwrap();
    let a = AcrossScience::new(lib, lib_dir.to_string_lossy().into(), tmp.join("local").to_string_lossy().into());
    a.set_on(true).unwrap();
    a.set_per_field(per_field).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let t0 = std::time::Instant::now();
    let r = a.run(now, 60).unwrap();
    println!("{r:?} in {:.1} s", t0.elapsed().as_secs_f64());
    let v = a.view(now, 60);
    println!("{}\n", v.subtitle);
    for p in &v.papers {
        let rec = a.state().items.into_iter().find(|x| x.key == p.key).unwrap();
        println!("{:>2}. [{}] {}\n    {} · {} · impact {:.2} · cited {}\n    {}\n", p.number, p.field_label, p.title, p.journal, p.date, rec.impact, rec.cited_by, p.line);
    }
    println!("quiet: {:?}", v.quiet_fields);
    if let Some(req) = a.digest_request() {
        println!("\ndigest prompt: {} chars", req.prompt.len());
    }
    println!("pool: {}", tmp.join("local").join(across::POOL_FILE).display());
}
