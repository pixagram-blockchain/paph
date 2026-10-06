//! Golden vectors (SPEC-004 §20), emitted BY the reference implementation so
//! the vectors and the code that must satisfy them cannot drift apart.  The
//! checked-in file `docs/golden/GOLDEN-004.json` is byte-for-byte this
//! module's output (a test asserts it), and the M4 JavaScript port consumes it
//! before porting begins — vectors first, port second.
//!
//! Everything here is integers and fixed-order strings: the JSON is built by
//! hand (zero dependencies, like everything else consensus-visible) with one
//! canonical rendering — no whitespace variation, no float, keys in the order
//! written below.

use crate::assignment::{assign, assign_sparse};
use crate::calibration::{Lut, Profile};
use crate::compare::Bag;
use crate::config::Config;
use crate::coverage::coverage;
use crate::local_v4::local_v4_bags;
use crate::nulls::{apply_local, apply_reverse, apply_shift, shift_offsets, LocalNull, LOCAL_FAMILY};
use crate::geom42::{
    correspond_42, diversity_42, geo_measure_42, gn_control_42, ham_cut, hamming, pack_all,
    pack_desc, strength_compat, vote_cells_42, Scratch,
};
use crate::keypoints::select_quality;
use crate::multimodel::{correspond_w, geo_measure_41, gn_control_41};
use crate::compare::mirror_side;
use crate::keypoints::Keypoint;
use crate::sha256::{hash_profile_id, hex, sha256};

fn arr_i64(v: &[i64]) -> String {
    let s: Vec<String> = v.iter().map(|x| x.to_string()).collect();
    format!("[{}]", s.join(","))
}

fn arr_i32(v: &[i32]) -> String {
    let s: Vec<String> = v.iter().map(|x| x.to_string()).collect();
    format!("[{}]", s.join(","))
}

fn arr_usize(v: &[usize]) -> String {
    let s: Vec<String> = v.iter().map(|x| x.to_string()).collect();
    format!("[{}]", s.join(","))
}

fn pairs(v: &[(usize, usize)]) -> String {
    let s: Vec<String> = v.iter().map(|(a, b)| format!("[{},{}]", a, b)).collect();
    format!("[{}]", s.join(","))
}

fn code_hex(hi: u32, lo: u32) -> String {
    format!("\"{:08x}{:08x}\"", hi, lo)
}

fn ln_name(n: LocalNull) -> &'static str {
    match n {
        LocalNull::Rot16 => "rot16",
        LocalNull::Rot32 => "rot32",
        LocalNull::Rot48 => "rot48",
        LocalNull::BitRev => "bitrev",
    }
}

/// Deterministic far-apart codes — the same stream the local_v4 tests use.
fn far(seed: u64, n: usize) -> Vec<(u32, u32)> {
    let mut s = seed.wrapping_mul(0x9e3779b97f4a7c15) | 1;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            ((s >> 32) as u32, s as u32)
        })
        .collect()
}

fn bag(codes: &[(u32, u32)]) -> Bag {
    let n = codes.len();
    Bag {
        hi: codes.iter().map(|c| c.0).collect(),
        lo: codes.iter().map(|c| c.1).collect(),
        x: (0..n).map(|i| (i as i64 * 65535) / n.max(1) as i64).collect(),
        y: (0..n).map(|i| (i as i64 * 65535) / n.max(1) as i64).collect(),
        n,
    }
}

fn bag_json(b: &Bag) -> String {
    let codes: Vec<String> = (0..b.n).map(|i| code_hex(b.hi[i], b.lo[i])).collect();
    format!(
        "{{\"codes\":[{}],\"x\":{},\"y\":{}}}",
        codes.join(","),
        arr_i64(&b.x),
        arr_i64(&b.y)
    )
}

fn local_case(name: &str, x: &Bag, y: &Bag, p: &Profile) -> String {
    let r = local_v4_bags(x, y, p);
    format!(
        "{{\"name\":\"{}\",\"a\":{},\"b\":{},\"expect\":{{\
\"matches\":{},\"w\":{},\"cap\":{},\"ctl_w\":{},\"ctl_n\":{},\"ctl_member\":\"{}\",\
\"lift_raw\":{},\"lift_ctl\":{},\"margin\":{},\"evidence\":{},\
\"prop_raw\":{},\"prop_ctl\":{},\"diversity\":{},\"d_a\":{},\"d_b\":{},\
\"pairs\":{},\"coverage_a_occupied\":{},\"coverage_a_concentration\":{}}}}}",
        name,
        bag_json(x),
        bag_json(y),
        r.matches, r.w, r.cap, r.ctl_w, r.ctl_n, r.ctl_member,
        r.lift_raw, r.lift_ctl, r.margin, r.evidence,
        r.prop_raw, r.prop_ctl, r.diversity, r.d_a, r.d_b,
        pairs(&r.pairs), r.coverage_a.occupied, r.coverage_a.concentration
    )
}

pub fn golden_json() -> String {
    let mut o = String::with_capacity(16 * 1024);
    o.push_str("{\n\"spec\":\"PAPH-SPEC-004.2\",\"milestone\":\"M8\",\"format\":3,\"comparator\":42,\"container\":3,\n");

    // ---- SHA-256 (FIPS 180-4 §7) ----
    o.push_str("\"sha256\":[");
    for (i, msg) in ["", "abc", "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"].iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!("{{\"msg\":\"{}\",\"digest\":\"{}\"}}", msg, hex(&sha256(msg.as_bytes()))));
    }
    o.push_str("],\n");

    // ---- hash-profile identity (§7) over the shipping defaults ----
    o.push_str(&format!(
        "\"hash_profile_id_default\":\"{}\",\n",
        hex(&hash_profile_id(&Config::default()))
    ));

    // ---- CAL-001-PROVISIONAL (§8, App B/C): the artefact IS the profile ----
    let p = Profile::cal001();
    let bytes = p.encode();
    o.push_str(&format!(
        "\"profile_cal001\":{{\"name\":\"{}\",\"len\":{},\"id\":\"{}\",\"bytes\":\"{}\"}},\n",
        p.name_str(),
        bytes.len(),
        hex(&p.id()),
        hex(&bytes)
    ));

    // ---- LUT evaluation (§8.3): integer interpolation, idiv truncation ----
    let shaped = Lut(vec![(0, 0), (2000, 8000), (10000, 10000)]);
    let xs: [i64; 8] = [0, 1, 1000, 1999, 2000, 6000, 9999, 10000];
    let ys: Vec<i64> = xs.iter().map(|&x| shaped.eval(x)).collect();
    o.push_str(&format!(
        "\"lut\":{{\"points\":[[0,0],[2000,8000],[10000,10000]],\"x\":{},\"y\":{}}},\n",
        arr_i64(&xs),
        arr_i64(&ys)
    ));

    // ---- assignment (§10.2, App A): outputs are the reference's own ----
    let cases: [(usize, usize, Vec<i32>); 6] = [
        (2, 2, vec![1, 2, 2, -1]),                         // the greedy-beater
        (2, 3, vec![3, 3, 3, 3, 3, 3]),                    // total tie → smallest columns
        (3, 2, vec![1, -1, 5, 2, -1, 1]),                  // tall: transposition path
        (3, 3, vec![-1, -1, -1, -1, -1, -1, -1, -1, -1]),  // no edges at all
        (4, 4, vec![7, 2, -1, 3, 2, 7, 3, -1, -1, 3, 7, 2, 3, -1, 2, 7]),
        (2, 4, vec![1, -1, 1, -1, -1, 1, 1, -1]),          // c3 empty: dummy-tie territory
    ];
    o.push_str("\"assignment\":[");
    for (i, (n, m, cost)) in cases.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let sol = assign(*n, *m, cost);
        let total: i64 = sol.iter().map(|&(r, c)| cost[r * m + c] as i64).sum();
        o.push_str(&format!(
            "{{\"n\":{},\"m\":{},\"cost\":{},\"pairs\":{},\"total\":{}}}",
            n, m, arr_i32(cost), pairs(&sol), total
        ));
    }
    o.push_str("],\n");

    // ---- SPEC-004.1 A2: the same cases on the edge-induced subgraph ----
    o.push_str("\"assignment_sparse\":[");
    for (i, (n, m, cost)) in cases.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let full = assign(*n, *m, cost);
        let sol = assign_sparse(*n, *m, cost);
        let total: i64 = sol.iter().map(|&(r, c)| cost[r * m + c] as i64).sum();
        o.push_str(&format!(
            "{{\"n\":{},\"m\":{},\"pairs\":{},\"total\":{},\"differs_from_full\":{}}}",
            n, m, pairs(&sol), total, sol != full
        ));
    }
    o.push_str("],\n");

    // ---- SPEC-004.1 A1: the weak signal on explicit keypoints ----
    {
        fn wdesc(seed: u32) -> [u32; 8] {
            let mut d = [0u32; 8];
            let mut s = seed;
            for w in d.iter_mut() {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                *w = s;
            }
            d
        }
        let kpn = |d: [u32; 8], x: i32, y: i32| Keypoint { desc: d, x, y, level: 0, sec: 0, s: 0 };
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in 0..5u32 {
            let d = wdesc(7000 + i);
            let (x, y) = (10000 + (i as i32) * 4200, 12000 + (i as i32) * 3100);
            a.push(kpn(d, x, y));
            b.push(kpn(d, x + 20000, y - 4000));
        }
        for i in 0..4i32 {
            let d = wdesc((7100 + i) as u32);
            a.push(kpn(d, 3000 + i * 9001 % 60000, 60000 - i * 7013 % 55000));
            b.push(kpn(d, 61000 - i * 8837 % 58000, 2000 + i * 6151 % 59000));
        }
        let cfg = Config::default();
        let am = mirror_side(&a, 65535);
        let pd = correspond_w(&a, &b);
        let pm = correspond_w(&am, &b);
        let (mm, measure, weak) = geo_measure_41(&a, &am, &b, &pd, &pm, &cfg, 256, 256, 6, 4);
        let (ctl, member) = gn_control_41(&a, &am, &b, &pd, &pm, &cfg, 256, 256, 6, 4);
        let side = |v: &Vec<Keypoint>| -> String {
            let s: Vec<String> = v
                .iter()
                .map(|k| {
                    let hexd: Vec<String> = k.desc.iter().map(|w| format!("{:08x}", w)).collect();
                    format!("{{\"d\":\"{}\",\"x\":{},\"y\":{}}}", hexd.join(""), k.x, k.y)
                })
                .collect();
            format!("[{}]", s.join(","))
        };
        o.push_str(&format!(
            "\"weak_geometry\":{{\"a\":{},\"b\":{},\"pool_direct\":{},\"pool_mirror\":{},\"models\":{},\"measure\":{},\"weak\":{},\"ctl\":{},\"ctl_member\":\"{}\"}},\n",
            side(&a), side(&b), pd.len(), pm.len(), mm.models.len(), measure, weak, ctl, member
        ));
    }

    // ---- SPEC-004.1 A3: CAL-003-PROPOSED, the container-2 anchor ----
    {
        let p = Profile::cal003();
        let b = p.encode();
        o.push_str(&format!(
            "\"profile_cal003\":{{\"name\":\"{}\",\"len\":{},\"id\":\"{}\",\"bytes\":\"{}\"}},\n",
            p.name_str(), b.len(), hex(&p.id()), hex(&b)
        ));
    }

    // ---- SPEC-004.2 Part II: CAL-004-PROPOSED, the container-3 anchor ----
    {
        let p = Profile::cal004();
        let b = p.encode();
        o.push_str(&format!(
            "\"profile_cal004\":{{\"name\":\"{}\",\"len\":{},\"id\":\"{}\",\"bytes\":\"{}\"}},\n",
            p.name_str(), b.len(), hex(&p.id()), hex(&b)
        ));
    }

    // ---- SPEC-004.2 §4: the early-abort matcher and the strength term ----
    {
        fn d42(seed: u32) -> [u32; 8] {
            let mut d = [0u32; 8];
            let mut s = seed;
            for w in d.iter_mut() {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                *w = s;
            }
            d
        }
        o.push_str("\"ham_cut\":[");
        let mut first = true;
        for i in 0..6u32 {
            for j in 0..3u32 {
                let (x, y) = (pack_desc(&d42(900 + i)), pack_desc(&d42(4000 + j)));
                let full = hamming(&x, &y);
                for lim in [full - 1, full, full + 1, 256] {
                    if !first {
                        o.push(',');
                    }
                    first = false;
                    let got = ham_cut(&x, &y, lim);
                    o.push_str(&format!(
                        "{{\"a\":{},\"b\":{},\"full\":{},\"limit\":{},\"exact\":{}}}",
                        900 + i, 4000 + j, full, lim, got <= lim
                    ));
                }
            }
        }
        o.push_str("],\n");

        o.push_str("\"strength_compat\":[");
        for (i, &(a, b)) in [(1u16, 1u16), (100, 100), (100, 75), (100, 50), (1000, 1),
                             (0, 500), (65535, 65535), (7, 64)].iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            o.push_str(&format!("{{\"sa\":{},\"sb\":{},\"sc\":{}}}", a, b, strength_compat(a, b)));
        }
        o.push_str("],\n");
    }

    // ---- SPEC-004.2 §5: soft scale binning, cells and kernel weights ----
    {
        let kpv = |x: i32, y: i32, level: u8, sec: u8| Keypoint {
            desc: [0u32; 8], x, y, level, sec, s: 1024,
        };
        o.push_str("\"vote_cells_42\":[");
        let cases: [(i32, i32, u8, u8, i32, i32, u8, u8, i64, i64, bool); 4] = [
            (10000, 10000, 0, 0, 20000, 10000, 0, 0, 256, 256, false),
            (10000, 10000, 0, 0, 20000, 10000, 0, 0, 256, 256, true),
            (10000, 12000, 1, 3, 30000, 9000, 0, 9, 512, 256, true),
            (40000, 40000, 2, 60, 5000, 60000, 1, 4, 256, 512, true),
        ];
        for (i, c) in cases.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            let a = kpv(c.0, c.1, c.2, c.3);
            let b = kpv(c.4, c.5, c.6, c.7);
            let vc = vote_cells_42(&a, &b, c.8, c.9, c.10);
            let keys: Vec<String> =
                vc.key[..vc.len as usize].iter().map(|k| k.to_string()).collect();
            let kws: Vec<String> =
                vc.kw[..vc.len as usize].iter().map(|k| k.to_string()).collect();
            o.push_str(&format!(
                "{{\"ax\":{},\"ay\":{},\"alevel\":{},\"asec\":{},\"bx\":{},\"by\":{},\"blevel\":{},\"bsec\":{},\
\"mda\":{},\"mdb\":{},\"soft\":{},\"keys\":[{}],\"kw\":[{}]}}",
                c.0, c.1, c.2, c.3, c.4, c.5, c.6, c.7, c.8, c.9, c.10,
                keys.join(","), kws.join(",")
            ));
        }
        o.push_str("],\n");
    }

    // ---- SPEC-004.2 §3: the quality selector, on a synthetic candidate set ----
    {
        // 40 candidates: 4 spatial clusters x 2 levels, half of them repeating
        // one descriptor exactly — which is the case the selector exists for.
        let mut all: Vec<Keypoint> = Vec::new();
        let shared = {
            let mut d = [0u32; 8];
            let mut s = 0x5eed_1234u32;
            for w in d.iter_mut() {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                *w = s;
            }
            d
        };
        for i in 0..40u32 {
            let mut d = shared;
            if i % 2 == 0 {
                let mut s = 0xabcd_0000u32.wrapping_add(i);
                for w in d.iter_mut() {
                    s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                    *w = s;
                }
            }
            all.push(Keypoint {
                desc: d,
                x: (((i % 4) as i32) * 16000 + 4000) as i32,
                y: ((((i / 4) % 4) as i32) * 16000 + 4000) as i32,
                level: (i % 3) as u8,
                sec: 0,
                s: (65535 - i * 700) as u16,
            });
        }
        // the selector consumes the pooled order, so pre-sort exactly as the
        // keypoint stage does
        all.sort_by(|a, b| {
            b.s.cmp(&a.s).then(a.level.cmp(&b.level)).then(a.y.cmp(&b.y)).then(a.x.cmp(&b.x))
        });
        let idx = |picked: &[Keypoint]| -> String {
            let v: Vec<String> = picked
                .iter()
                .map(|k| {
                    let p = all.iter().position(|q| {
                        q.x == k.x && q.y == k.y && q.level == k.level && q.desc == k.desc
                    });
                    p.unwrap_or(usize::MAX).to_string()
                })
                .collect();
            format!("[{}]", v.join(","))
        };
        o.push_str("\"select_quality\":[");
        for (i, want) in [4usize, 8, 16].iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            o.push_str(&format!("{{\"want\":{},\"picked\":{}}}", want, idx(&select_quality(&all, *want))));
        }
        o.push_str("],\n");
    }

    // ---- SPEC-004.2 §7–§8: extraction, exclusion and the diversity channel ----
    {
        fn d42b(seed: u32) -> [u32; 8] {
            let mut d = [0u32; 8];
            let mut s = seed;
            for w in d.iter_mut() {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                *w = s;
            }
            d
        }
        let kpn = |d: [u32; 8], x: i32, y: i32, level: u8, st: u16| Keypoint {
            desc: d, x, y, level, sec: 0, s: st,
        };
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in 0..12u32 {
            let d = d42b(31_000 + i);
            let (x, y) = (6000 + ((i as i32) * 2313) % 6000, 6000 + ((i as i32) * 3517) % 6000);
            a.push(kpn(d, x, y, (i % 2) as u8, 1024 + (i as u16) * 13));
            b.push(kpn(d, x + 9000, y + 2500, (i % 2) as u8, 1100 + (i as u16) * 11));
        }
        for i in 0..10u32 {
            let d = d42b(32_000 + i);
            let (x, y) = (20000 + ((i as i32) * 2401) % 6000, 20000 + ((i as i32) * 3269) % 6000);
            a.push(kpn(d, x, y, 0, 900 + (i as u16) * 7));
            b.push(kpn(d, x - 8000, y + 6000, 0, 950 + (i as u16) * 5));
        }
        let cfg = Config::default();
        let p = Profile::cal004();
        let am = mirror_side(&a, 65535);
        let (ad, amd, bd) = (pack_all(&a), pack_all(&am), pack_all(&b));
        let pd = correspond_42(&a, &ad, &b, &bd, &p);
        let pm = correspond_42(&am, &amd, &b, &bd, &p);
        let mut sc = Scratch::new();
        let (mm, measure, weak) =
            geo_measure_42(&a, &am, &b, &pd, &pm, &cfg, &p, 256, 256, &mut sc);
        let (ctl, member) = gn_control_42(&a, &am, &b, &pd, &pm, &cfg, &p, 256, 256, &mut sc);
        let (div, cov) = diversity_42(&mm, &a, &b, &p);
        let side = |v: &Vec<Keypoint>| -> String {
            let s: Vec<String> = v
                .iter()
                .map(|k| {
                    let hexd: Vec<String> = k.desc.iter().map(|w| format!("{:08x}", w)).collect();
                    format!(
                        "{{\"d\":\"{}\",\"x\":{},\"y\":{},\"level\":{},\"s\":{}}}",
                        hexd.join(""), k.x, k.y, k.level, k.s
                    )
                })
                .collect();
            format!("[{}]", s.join(","))
        };
        let models: Vec<String> = mm
            .models
            .iter()
            .map(|m| {
                format!(
                    "{{\"inliers\":{},\"mirror\":{},\"scaleQ16\":{},\"medianErr\":{},\"confSum\":{}}}",
                    m.inliers, m.mirror, m.scale_q16, m.median_err, m.conf_sum
                )
            })
            .collect();
        o.push_str(&format!(
            "\"geometry_42\":{{\"a\":{},\"b\":{},\"pool_direct\":{},\"pool_mirror\":{},\
\"models\":[{}],\"measure\":{},\"weak\":{},\"ctl\":{},\"ctl_member\":\"{}\",\
\"diversity\":{{\"spatial\":{},\"scale\":{},\"model\":{},\"descriptor\":{},\"combined\":{},\"multiplier\":{}}},\
\"coverage_occupied\":{},\"coverage\":{}}},\n",
            side(&a), side(&b), pd.len(), pm.len(), models.join(","), measure, weak, ctl, member,
            div.spatial, div.scale, div.model, div.descriptor, div.combined, div.multiplier,
            cov.occupied, cov.coverage
        ));
    }

    // ---- LN maps (§9.2, App D) ----
    let samples: [(u32, u32); 4] = [(0x12345678, 0x9abcdef0), (1, 0), (0, 1), (0xffffffff, 0)];
    o.push_str("\"nulls_local\":[");
    for (i, &(hi, lo)) in samples.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!("{{\"code\":{}", code_hex(hi, lo)));
        for f in LOCAL_FAMILY {
            let (h2, l2) = apply_local(f, hi, lo);
            o.push_str(&format!(",\"{}\":{}", ln_name(f), code_hex(h2, l2)));
        }
        o.push('}');
    }
    o.push_str("],\n");

    // ---- GN offsets and permutations (§9.3, App D) ----
    o.push_str("\"shift_offsets\":{");
    for (i, n) in [2usize, 6, 12, 50, 128].iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!("\"{}\":{}", n, arr_usize(&shift_offsets(*n))));
    }
    o.push_str("},\n");
    let corr: Vec<(usize, usize, i32)> = (0..7).map(|i| (i, 100 + i, i as i32)).collect();
    let shifted = apply_shift(&corr, 3);
    let reversed = apply_reverse(&corr);
    let b_of = |v: &[(usize, usize, i32)]| -> Vec<usize> { v.iter().map(|c| c.1).collect() };
    o.push_str(&format!(
        "\"shift_apply\":{{\"n\":7,\"b\":{},\"shift3_b\":{},\"reverse_b\":{}}},\n",
        arr_usize(&b_of(&corr)),
        arr_usize(&b_of(&shifted)),
        arr_usize(&b_of(&reversed))
    ));

    // ---- coverage (§11): cell(x) = (x·g)>>16, 4-connected concentration ----
    let at = |cx: i64, cy: i64| -> (i64, i64) { (cx * 16384 + 8192, cy * 16384 + 8192) };
    let cov_cases: [(&str, Vec<(i64, i64)>); 3] = [
        ("empty", vec![]),
        ("l_shape", vec![at(0, 0), at(0, 1), at(1, 1)]),
        ("two_blobs", vec![at(0, 0), at(1, 0), at(0, 1), at(3, 3)]),
    ];
    o.push_str("\"coverage\":[");
    for (i, (name, pts)) in cov_cases.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let c = coverage(pts, 4);
        let flat: Vec<i64> = pts.iter().flat_map(|&(x, y)| [x, y]).collect();
        o.push_str(&format!(
            "{{\"name\":\"{}\",\"g\":4,\"points\":{},\"occupied\":{},\"coverage\":{},\"bbox_cells\":{},\"concentration\":{}}}",
            name, arr_i64(&flat), c.occupied, c.coverage, c.bbox_cells, c.concentration
        ));
    }
    o.push_str("],\n");

    // ---- the rebuilt local channel (§10), on explicit bags ----
    let p = Profile::cal001();
    let self_codes = far(1, 8);
    let mut near = self_codes.clone();
    near[0].0 ^= 3; // two bits: still within hamming_t of its partner
    near[3].1 ^= 1;
    let wall: Vec<(u32, u32)> = std::iter::repeat((0xdeadbeef, 0x01234567)).take(8).collect();
    o.push_str("\"local_v4\":[");
    o.push_str(&local_case("self", &bag(&self_codes), &bag(&self_codes), &p));
    o.push(',');
    o.push_str(&local_case("near", &bag(&self_codes), &bag(&near), &p));
    o.push(',');
    o.push_str(&local_case("wall_of_tiles", &bag(&wall), &bag(&wall), &p));
    o.push_str("],\n");

    // ---- §12.2 confidence table ----
    let cd: [(i32, i32); 8] =
        [(0, 999), (0, 64), (0, 1), (1, 2), (10, 20), (40, 88), (88, 88), (5, 999)];
    o.push_str("\"conf\":[");
    for (i, (d1, d2)) in cd.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&format!(
            "{{\"d1\":{},\"d2\":{},\"conf\":{}}}",
            d1,
            d2,
            crate::multimodel::conf(*d1, *d2)
        ));
    }
    o.push_str("],\n");

    // ---- §14 lattice cases, straight through the reference ----
    o.push_str("\"lattice\":[");
    let latcase = |name: &str, x: &crate::lattice::LatticeIn| -> String {
        let v = crate::lattice::lattice_v4(x, &Profile::cal001());
        let ch: Vec<String> = x
            .channels
            .iter()
            .map(|(n, val, m)| format!("[\"{}\",{},{}]", n, val, m))
            .collect();
        let basis: Vec<String> = v.basis.iter().map(|b| format!("\"{}\"", b)).collect();
        format!(
            "{{\"name\":\"{}\",\"in\":{{\"identical\":{},\"channels\":[{}],\"geo_measurable\":{},\"geo_evidence\":{},\"total_inliers\":{},\"topology\":{},\"diversity\":{},\"coverage_min\":{},\"mirror\":{}}},\"expect\":{{\"state\":\"{}\",\"basis\":[{}],\"structural\":{},\"certifiable\":{}}}}}",
            name,
            x.identical,
            ch.join(","),
            x.geo_measurable,
            x.geo_evidence,
            x.total_inliers,
            x.topology_class,
            x.diversity,
            x.coverage_min,
            x.any_mirror_model,
            v.state,
            basis.join(","),
            v.structural,
            v.certifiable
        )
    };
    let mk = |vals: [i64; 7], meas: [bool; 7]| -> [(&'static str, i64, bool); 7] {
        let names = ["dct", "local", "shape", "topology", "runs", "palette", "silhouette"];
        let mut c: [(&'static str, i64, bool); 7] = [("", 0, false); 7];
        for k in 0..7 {
            c[k] = (names[k], vals[k], meas[k]);
        }
        c
    };
    use crate::lattice::LatticeIn;
    let cases: [(&str, LatticeIn); 5] = [
        ("copy_both", LatticeIn {
            identical: false,
            channels: mk([6000; 7], [true; 7]),
            geo_measurable: true, geo_evidence: 4000, total_inliers: 20,
            topology_class: 4, diversity: 8000, coverage_min: 5000,
            any_mirror_model: false,
        }),
        ("r1_two_secondaries", LatticeIn {
            identical: false,
            channels: mk([9000; 7], [true, true, true, false, false, false, false]),
            geo_measurable: false, geo_evidence: 0, total_inliers: 0,
            topology_class: 0, diversity: 8000, coverage_min: 5000,
            any_mirror_model: false,
        }),
        ("r2_wall_of_tiles", LatticeIn {
            identical: false,
            channels: mk([7500; 7], [true; 7]),
            geo_measurable: false, geo_evidence: 0, total_inliers: 0,
            topology_class: 0, diversity: 800, coverage_min: 5000,
            any_mirror_model: false,
        }),
        ("r3_scattered_geometry", LatticeIn {
            identical: false,
            channels: mk([500; 7], [true; 7]),
            geo_measurable: true, geo_evidence: 4000, total_inliers: 20,
            topology_class: 1, diversity: 8000, coverage_min: 5000,
            any_mirror_model: false,
        }),
        ("r4_no_spatial_support", LatticeIn {
            identical: false,
            channels: mk([7500; 7], [true; 7]),
            geo_measurable: false, geo_evidence: 0, total_inliers: 0,
            topology_class: 0, diversity: 8000, coverage_min: 1200,
            any_mirror_model: false,
        }),
    ];
    for (i, (name, x)) in cases.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str(&latcase(name, x));
    }
    o.push_str("],\n");

    // ---- §16 constants, for the record ----
    o.push_str(&format!(
        "\"limits\":{{\"max_width\":{},\"max_height\":{},\"max_pixels\":{}}}\n",
        crate::wire::MAX_WIDTH,
        crate::wire::MAX_HEIGHT,
        crate::wire::MAX_PIXELS
    ));
    o.push_str("}\n");
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emission_is_deterministic() {
        let a = golden_json();
        let b = golden_json();
        assert_eq!(a, b);
        assert!(a.contains("\"assignment\""));
        assert!(a.contains("\"local_v4\""));
        assert!(a.contains("\"bitrev\""));
    }

    /// The file in the repository is exactly what the reference emits.  If
    /// this fails, regenerate: `printf G | cargo run --bin paphcli > \
    /// docs/golden/GOLDEN-004.json` — never edit the file by hand.
    #[test]
    fn checked_in_file_matches_reference() {
        let disk = include_str!("../../docs/golden/GOLDEN-004.json");
        assert_eq!(disk, golden_json(), "docs/golden/GOLDEN-004.json is stale");
    }
}
