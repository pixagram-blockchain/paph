//! PAPH-X on wires from files — the native side of the cross-engine checks
//! (test/x-wasm.mjs runs the same calls through the WebAssembly glue and
//! diffs the text).
//!
//!     xcli route   a.t1 a.t2                       route record, hex
//!     xcli screen  a.t1 a.t2 b.t1 b.t2             screen record, JSON
//!     xcli compare a.t1 a.t2 b.t1 b.t2 [flags]     the X report, JSON (flags as paph_xcompare)
//!     xcli rank    q.t1 q.t2 c1.t1 c1.t2 ... [--flags n]   rank records, one line each
//!     xcli sidecar a.t1 a.t2 out.pax1              write the sidecar, then report through it
//!     xcli sisig   a.t1 a.t2                       PAPH-SI signature, hex, then its posting keys
//!     xcli siplan  q.t1 q.t2                       the SQL plan of a PAPH-SI query, JSON
//!     xcli sirank  q.t1 q.t2 c1.t1 c1.t2 ... [--threshold n] [--budget n]
//!                                                  every candidate's score, then the index's answer
//!
//! A missing Tier 2 is spelled `-`.
use paph::calibration::Profile;
use paph::config::Config;
use paph::prepared::Prepared;
use paph::x::compare::{report_json, xcompare, xscreen, Scope, XCtx, XOptions};
use paph::x::profile::{POLICY_EXACT, POLICY_FAST, POLICY_SAFE};
use paph::x::rank::{xrank, RankScratch, XRankOptions, XRANK_FIELDS};
use paph::x::{XBound, XPrepared};

fn wires(t1: &str, t2: &str) -> (Vec<u8>, Option<Vec<u8>>) {
    let a = std::fs::read(t1).expect("tier 1");
    let b = if t2 == "-" { None } else { Some(std::fs::read(t2).expect("tier 2")) };
    (a, b)
}

fn side(xb: &XBound, t1: &str, t2: &str) -> XPrepared {
    let (a, b) = wires(t1, t2);
    XPrepared::new(Prepared::new(&a, b.as_deref()).expect("tier 1 refused"), xb)
}

fn policy_of(flags: u32) -> Option<u8> {
    match flags & 3 {
        1 => Some(POLICY_FAST),
        2 => Some(POLICY_SAFE),
        3 => Some(POLICY_EXACT),
        _ => None,
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let xb = XBound::new(Profile::cal004(), paph::x::XProfile::x1());
    let cfg = Config::default();
    let mut ctx = XCtx::new();
    match a[1].as_str() {
        "route" => {
            let s = side(&xb, &a[2], &a[3]);
            println!("{}", s.route.to_bytes().iter().map(|b| format!("{:02x}", b)).collect::<String>());
        }
        "screen" => {
            let (x, y) = (side(&xb, &a[2], &a[3]), side(&xb, &a[4], &a[5]));
            let s = xscreen(&x, &y, &cfg, &xb, &mut ctx);
            println!("{{\"state\":{},\"local\":{},\"band\":{},\"global\":{},\"poolDirect\":{},\"poolMirror\":{},\"rows\":{},\"swapped\":{}}}", s.state.code(), s.route.local, s.route.band, s.route.global, s.direct.count, s.mirror.count, s.stats.rows, s.swapped);
        }
        "compare" => {
            let (x, y) = (side(&xb, &a[2], &a[3]), side(&xb, &a[4], &a[5]));
            let flags: u32 = a.get(6).map(|f| f.parse().unwrap()).unwrap_or(0);
            let opts = XOptions { policy: policy_of(flags), audit: flags & 4 != 0, scope: if flags & 8 != 0 { Scope::Copy } else { Scope::Full } };
            let r = xcompare(&x, &y, &cfg, &xb, &opts, &mut ctx);
            println!("{}", report_json(&r).to_string());
        }
        "rank" => {
            let mut flags = 0u32;
            let mut files: Vec<&String> = Vec::new();
            let mut i = 2;
            while i < a.len() {
                if a[i] == "--flags" {
                    flags = a[i + 1].parse().unwrap();
                    i += 2;
                } else {
                    files.push(&a[i]);
                    i += 1;
                }
            }
            let q = side(&xb, files[0], files[1]);
            let cands: Vec<XPrepared> = files[2..].chunks(2).map(|c| side(&xb, c[0], c[1])).collect();
            let refs: Vec<Option<&XPrepared>> = cands.iter().map(Some).collect();
            let mut out = vec![0i32; refs.len() * XRANK_FIELDS];
            let mut rs = RankScratch::new();
            let opts = XRankOptions { policy: policy_of(flags), gate: flags & 4 == 0, scope: if flags & 8 != 0 { Scope::Full } else { Scope::Copy } };
            xrank(&q, &refs, &cfg, &xb, &opts, &mut ctx, &mut rs, &mut out);
            for k in 0..refs.len() {
                println!("{}", out[k * XRANK_FIELDS..(k + 1) * XRANK_FIELDS].iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","));
            }
        }
        "sidecar" => {
            let s = side(&xb, &a[2], &a[3]);
            let b = paph::x::sidecar::encode(&s);
            std::fs::write(&a[4], &b).unwrap();
            let (t1, t2) = wires(&a[2], &a[3]);
            let p = Prepared::new(&t1, t2.as_deref()).unwrap();
            let t = paph::x::sidecar::decode(p, &xb, &b).ok().expect("sidecar decodes");
            println!("{} bytes, route {}, order {}", b.len(), t.route == s.route, t.order == s.order);
        }
        "sisig" => {
            let sp = paph::x::si::SiProfile::si1();
            let s = side(&xb, &a[2], &a[3]);
            let sig = paph::x::si::SiSig::build(&s.p, &s.route, &sp);
            let mut k = [0i32; paph::x::si::abi::MAX_KEYS];
            let n = paph::x::si::abi::sig_keys(&sig, &mut k);
            println!("{}", sig.to_bytes().iter().map(|b| format!("{:02x}", b)).collect::<String>());
            println!("{}", k[..n].iter().map(|v| v.to_string()).collect::<Vec<_>>().join(","));
        }
        "siplan" => {
            let sp = paph::x::si::abi::SiBound { prof: paph::x::si::SiProfile::si1(), id: paph::x::si::SiProfile::si1().id() };
            let s = side(&xb, &a[2], &a[3]);
            let q = paph::x::si::SiQuery::new(&s.p, &s.route, &sp.prof);
            println!("{}", paph::x::si::abi::plan_json(&sp, &q).to_string());
        }
        "sirank" => {
            let sp = paph::x::si::SiProfile::si1();
            let (mut th, mut budget) = (sp.threshold, sp.budget as usize);
            let mut files: Vec<&String> = Vec::new();
            let mut i = 2;
            while i < a.len() {
                match a[i].as_str() {
                    "--threshold" => {
                        th = a[i + 1].parse().unwrap();
                        i += 2;
                    }
                    "--budget" => {
                        budget = a[i + 1].parse().unwrap();
                        i += 2;
                    }
                    _ => {
                        files.push(&a[i]);
                        i += 1;
                    }
                }
            }
            let q = side(&xb, files[0], files[1]);
            let qy = paph::x::si::SiQuery::new(&q.p, &q.route, &sp);
            let mut idx = paph::x::si::SiIndex::new();
            let mut scores = Vec::new();
            for c in files[2..].chunks(2) {
                let s = side(&xb, c[0], c[1]);
                let sig = paph::x::si::SiSig::build(&s.p, &s.route, &sp);
                scores.push(if qy.touches(&sig) { qy.score(&sig).to_string() } else { "-".into() });
                idx.add(sig);
            }
            println!("{}", scores.join(","));
            let mut out = Vec::new();
            let st = idx.query(&qy, th, budget, &mut out);
            println!("{}", out.iter().map(|(s, v)| format!("{s}:{v}")).collect::<Vec<_>>().join(","));
            println!("{},{},{}", st.touched, st.admitted, st.postings);
        }
        _ => panic!("mode"),
    }
}
