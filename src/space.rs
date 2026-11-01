//! Espaço em disco a partir do índice. O índice está ordenado pelo caminho normalizado, então cada pasta é
//! um trecho contínuo dele: com a soma acumulada dos tamanhos, o tamanho de qualquer pasta sai na hora.
use crate::{Entry, Kind, normalize};
use std::path::{Path, PathBuf};

/// `cum[i]` = soma dos bytes de `entries[..i]`.
pub struct Sizes {
    cum: Vec<u64>,
}
impl Sizes {
    pub fn new(entries: &[Entry]) -> Self {
        let mut cum = Vec::with_capacity(entries.len() + 1);
        let mut total = 0u64;
        cum.push(0);
        for e in entries {
            total += e.bytes;
            cum.push(total);
        }
        Self { cum }
    }
    pub fn sum(&self, lo: usize, hi: usize) -> u64 {
        self.cum[hi] - self.cum[lo]
    }
}

/// Chave de busca de uma pasta: caminho normalizado terminado em `\`.
pub fn prefix_of(folder: &Path) -> String {
    let mut p = normalize(&folder.to_string_lossy());
    if !p.ends_with('\\') {
        p.push('\\');
    }
    p
}
/// Trecho do índice com tudo que está dentro da pasta (em qualquer nível).
pub fn range(entries: &[Entry], prefix: &str) -> (usize, usize) {
    let lo = entries.partition_point(|e| &*e.searchable < prefix);
    let hi = lo + entries[lo..].partition_point(|e| e.searchable.starts_with(prefix));
    (lo, hi)
}

pub struct Child {
    pub name: String,  // grafia original
    pub path: PathBuf, // caminho completo
    pub is_dir: bool,
    pub bytes: u64,
    pub files: usize,
    pub lo: usize,
    pub hi: usize,
}
/// Subpastas e arquivos diretos de uma pasta, do maior para o menor. Pula cada subpasta inteira de uma vez,
/// então custa pouco mesmo na raiz de um disco com milhões de arquivos.
pub fn children(entries: &[Entry], sizes: &Sizes, folder: &Path) -> Vec<Child> {
    let prefix = prefix_of(folder);
    let depth = prefix.matches('\\').count();
    let (lo, hi) = range(entries, &prefix);
    let mut out = Vec::new();
    let mut i = lo;
    while i < hi {
        let s = &entries[i].searchable;
        let name = entries[i].path_str().split(['\\', '/']).nth(depth).unwrap_or("").to_owned();
        let base = folder.join(&name);
        match s[prefix.len()..].find('\\') {
            Some(p) => {
                let sub = &s[..prefix.len() + p + 1];
                let end = i + entries[i..hi].partition_point(|e| e.searchable.starts_with(sub));
                out.push(Child { name, path: base, is_dir: true, bytes: sizes.sum(i, end), files: end - i, lo: i, hi: end });
                i = end;
            }
            None => {
                out.push(Child { name, path: base, is_dir: false, bytes: entries[i].bytes, files: 1, lo: i, hi: i + 1 });
                i += 1;
            }
        }
    }
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Quanto cada categoria ocupa num trecho do índice, da maior para a menor.
pub fn by_kind(entries: &[Entry]) -> Vec<(Kind, u64, usize)> {
    let mut sums = [(0u64, 0usize); Kind::ALL.len()];
    for e in entries {
        let k = Kind::ALL.iter().position(|k| *k == e.kind).unwrap_or(Kind::ALL.len() - 1);
        sums[k].0 += e.bytes;
        sums[k].1 += 1;
    }
    let mut out: Vec<_> = Kind::ALL.iter().zip(sums).filter(|(_, s)| s.1 > 0).map(|(k, s)| (*k, s.0, s.1)).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}
/// Bytes de arquivos criados a partir de `since` (segundos desde 1970).
pub fn created_since(entries: &[Entry], since: u64) -> u64 {
    entries.iter().filter(|e| e.created >= since).map(|e| e.bytes).sum()
}

/// Treemap "squarified": divide `(x, y, w, h)` em retângulos com área proporcional a `sizes` (já em ordem
/// decrescente), o mais perto possível de quadrados.
pub fn squarify(sizes: &[u64], rect: (f32, f32, f32, f32)) -> Vec<(f32, f32, f32, f32)> {
    let total: f64 = sizes.iter().map(|&s| s as f64).sum();
    let (x, y, w, h) = rect;
    if total <= 0. || w <= 0. || h <= 0. {
        return vec![(x, y, 0., 0.); sizes.len()];
    }
    let scale = (w as f64 * h as f64) / total;
    let areas: Vec<f64> = sizes.iter().map(|&s| s as f64 * scale).collect();
    let mut out = Vec::with_capacity(sizes.len());
    let (mut x, mut y, mut w, mut h) = (x as f64, y as f64, w as f64, h as f64);
    let worst = |row: &[f64], side: f64| -> f64 {
        let s: f64 = row.iter().sum();
        let (max, min) = row.iter().fold((0f64, f64::MAX), |(a, b), &r| (a.max(r), b.min(r)));
        if s <= 0. || min <= 0. {
            return f64::MAX;
        }
        (side * side * max / (s * s)).max(s * s / (side * side * min))
    };
    let mut i = 0;
    while i < areas.len() {
        let side = w.min(h);
        let mut j = i + 1;
        while j < areas.len() && worst(&areas[i..=j], side) <= worst(&areas[i..j], side) {
            j += 1;
        }
        let row = &areas[i..j];
        let s: f64 = row.iter().sum();
        if w >= h {
            // coluna à esquerda
            let cw = if h > 0. { s / h } else { 0. };
            let mut yy = y;
            for &a in row {
                let rh = if cw > 0. { a / cw } else { 0. };
                out.push((x as f32, yy as f32, cw as f32, rh as f32));
                yy += rh;
            }
            x += cw;
            w -= cw;
        } else {
            // linha em cima
            let rh = if w > 0. { s / w } else { 0. };
            let mut xx = x;
            for &a in row {
                let cw = if rh > 0. { a / rh } else { 0. };
                out.push((xx as f32, y as f32, cw as f32, rh as f32));
                xx += cw;
            }
            y += rh;
            h -= rh;
        }
        i = j;
    }
    out
}

/// Algo sugerido para apagar, com o que é e por que foi sugerido.
#[derive(Clone, Debug)]
pub struct Item {
    pub path: PathBuf,
    pub bytes: u64,
    pub date: u64, // última modificação de algo dentro dele
    pub what: String,
    pub why: String,
}
/// Lixo e caches: arquivos cobertos por uma regra "pode apagar", juntados no item que a regra indica
/// (o conteúdo de Temp, uma pasta node_modules...). Só itens de 1 MB ou mais, do maior para o menor.
pub fn junk(entries: &[Entry]) -> Vec<Item> {
    use rayon::prelude::*;
    let found: Vec<(String, &'static crate::knowledge::Rule, u64, u64)> = entries
        .par_iter()
        .filter_map(|e| {
            let (rule, n) = crate::knowledge::junk_item(&e.searchable)?;
            let item: Vec<&str> = e.path_str().split(['\\', '/']).take(n + 1).collect();
            Some((item.join("\\"), rule, e.bytes, e.modified))
        })
        .collect();
    let mut map: std::collections::HashMap<String, (&'static crate::knowledge::Rule, u64, u64)> = std::collections::HashMap::new();
    for (path, rule, bytes, modified) in found {
        let e = map.entry(path).or_insert((rule, 0, 0));
        e.1 += bytes;
        e.2 = e.2.max(modified);
    }
    let has = |key: &str| {
        let i = entries.partition_point(|e| &*e.searchable < key);
        entries.get(i).is_some_and(|e| &*e.searchable == key)
    };
    let mut list: Vec<(String, String, &'static crate::knowledge::Rule)> = map
        .into_iter()
        .filter(|(p, _)| crate::knowledge::protected(Path::new(p), false).is_none())
        // dentro de um jogo instalado ou de um app Electron, uma pasta "cache" ou "node_modules" pode ser parte dele
        .filter(|(p, _)| {
            let k = normalize(p);
            !["\\steamapps\\common\\", "\\epic games\\", "\\riot games\\", "\\resources\\app", "\\mods\\"].iter().any(|s| k.contains(s))
        })
        // node_modules só de projeto (com package.json ao lado); a de um programa instalado faz parte dele
        .filter(|(p, v)| v.0.pat != "~node_modules" || Path::new(p).parent().is_some_and(|d| has(&normalize(&d.join("package.json").to_string_lossy()))))
        .map(|(p, v)| (normalize(&p), p, v.0))
        .collect();
    // Um item dentro de outro sai: apagar o de fora já leva o de dentro, e o tamanho tem que dizer isso.
    list.sort_by(|a, b| a.0.cmp(&b.0));
    let mut kept: Vec<(String, String, &'static crate::knowledge::Rule)> = Vec::new();
    for it in list {
        if kept.last().is_some_and(|k| it.0.starts_with(&format!("{}\\", k.0))) {
            continue;
        }
        kept.push(it);
    }
    // Tamanho de verdade: tudo que está dentro da pasta, não só a parte que a regra cobriu.
    let mut out: Vec<Item> = kept
        .into_iter()
        .filter_map(|(key, p, rule)| {
            let (lo, hi) = range(entries, &format!("{key}\\"));
            // modelo de IA dentro (voz do RVC, checkpoint...): pode ter sido treinado pela pessoa e não volta
            // (de 1 MB para cima: pacotes Python trazem .pth minúsculos que só listam caminhos)
            let model = |e: &Entry| e.bytes >= 1 << 20 && crate::knowledge::MODEL_EXTS.contains(&e.ext());
            let single = (hi == lo).then(|| entries.partition_point(|e| *e.searchable < *key)).and_then(|i| entries.get(i)).filter(|e| *e.searchable == *key);
            if (entries[lo..hi].iter().any(model) || single.is_some_and(model)) && !crate::knowledge::downloads_models(rule) {
                return None;
            }
            let (bytes, date) = if hi > lo {
                (entries[lo..hi].iter().map(|e| e.bytes).sum(), entries[lo..hi].iter().map(|e| e.modified).max().unwrap_or(0))
            } else {
                let i = entries.partition_point(|e| *e.searchable < *key);
                entries.get(i).filter(|e| *e.searchable == *key).map_or((0, 0), |e| (e.bytes, e.modified))
            };
            Some(Item { path: p.into(), bytes, date, what: rule.what.into(), why: rule.why.into() })
        })
        .filter(|i| i.bytes >= 1 << 20)
        .collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    out
}
const DAY: u64 = 86_400;
/// Instaladores e pacotes baixados há mais de 30 dias, na pasta Downloads.
pub fn old_installers(entries: &[Entry], now: u64) -> Vec<Item> {
    const EXTS: [&str; 10] = ["exe", "msi", "msix", "appx", "iso", "zip", "rar", "7z", "apk", "dmg"];
    let mut out: Vec<Item> = entries
        .iter()
        .filter(|e| e.searchable.contains("\\downloads\\") && EXTS.contains(&e.ext()) && e.modified > 0 && e.modified + 30 * DAY < now)
        .filter(|e| crate::knowledge::protected(e.path(), false).is_none())
        .map(|e| Item {
            path: e.path().to_path_buf(),
            bytes: e.bytes,
            date: e.modified,
            what: if matches!(e.ext(), "exe" | "msi" | "msix" | "appx" | "apk" | "dmg") { "Instalador baixado" } else { "Arquivo compactado ou imagem de disco baixada" }.into(),
            why: format!("Está em Downloads e não muda há {} dias.", (now - e.modified) / DAY),
        })
        .collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    out
}
/// Arquivos de 500 MB ou mais que não mudam há mais de 6 meses (fora do sistema, de jogos e do lixo).
pub fn forgotten(entries: &[Entry], now: u64) -> Vec<Item> {
    use crate::knowledge::{Seal, explain_key};
    let mut out: Vec<Item> = entries
        .iter()
        // só mídia, documentos e pacotes: DLL, executável ou dado de programa grande faz parte de algo instalado
        .filter(|e| matches!(e.kind, Kind::Video | Kind::Audio | Kind::Photo | Kind::Archive | Kind::Disk | Kind::Document | Kind::Design))
        .filter(|e| e.bytes >= 500 << 20 && e.modified > 0 && e.modified + 180 * DAY < now)
        .filter(|e| explain_key(&e.searchable).is_none_or(|r| r.seal == Seal::Info))
        .filter(|e| crate::knowledge::protected(e.path(), false).is_none())
        .map(|e| Item {
            path: e.path().to_path_buf(),
            bytes: e.bytes,
            date: e.modified,
            what: e.kind.label().trim_end_matches('s').to_owned(),
            why: format!("Grande e sem mudar há {} meses.", (now - e.modified) / (30 * DAY)),
        })
        .collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes));
    out
}

#[cfg(test)]
mod tests {
    /// Conferência à mão: `SEEK_INDEX=arquivo.velo SEEK_ROOT=D:\ cargo test --release -- --ignored real_index --nocapture`
    #[test]
    #[ignore]
    fn real_index_suggestions() {
        use std::sync::atomic::{AtomicBool, AtomicUsize};
        let (Ok(file), Ok(root)) = (std::env::var("SEEK_INDEX"), std::env::var("SEEK_ROOT")) else { return };
        let scan = crate::cache::load(Path::new(&file), Path::new(&root), &AtomicBool::new(false), &AtomicUsize::new(0)).unwrap();
        let now = crate::clock::now_secs();
        for (title, list) in [("LIXO", junk(&scan.entries)), ("INSTALADORES", old_installers(&scan.entries, now)), ("ESQUECIDOS", forgotten(&scan.entries, now))] {
            println!("== {title}: {} itens, {} MB", list.len(), list.iter().map(|i| i.bytes).sum::<u64>() >> 20);
            for i in list.iter().take(25) {
                println!("{:>8} MB  {}  [{}]", i.bytes >> 20, i.path.display(), i.what);
            }
        }
    }
    #[test]
    fn junk_groups_files_into_the_right_items() {
        let e = idx(&[
            ("C:\\Users\\Edu\\AppData\\Local\\Temp\\a\\1.bin", 2 << 20),
            ("C:\\Users\\Edu\\AppData\\Local\\Temp\\a\\2.bin", 2 << 20),
            ("C:\\Users\\Edu\\AppData\\Local\\Temp\\b.tmp", 3 << 20),
            ("C:\\proj\\node_modules\\x\\node_modules\\y.js", 5 << 20),
            ("C:\\proj\\package.json", 1),
            ("C:\\App\\resources\\node_modules\\z.js", 7 << 20), // sem package.json ao lado: é de um programa
            ("C:\\Cursor\\resources\\app\\node_modules\\z.js", 7 << 20), // app Electron (tem package.json)
            ("C:\\Cursor\\resources\\app\\package.json", 1),
            ("D:\\SteamLibrary\\steamapps\\common\\Jogo\\cache\\c.bin", 7 << 20), // parte de um jogo
            ("C:\\Users\\Edu\\Cache\\a.bin", 2 << 20), // solto num cache feito à mão: fica
            ("C:\\Users\\Edu\\Cache\\.nuget\\packages\\p.nupkg", 6 << 20), // regra própria, mais funda
            ("D:\\cache\\hf\\m\\model.safetensors", 11 << 20), // modelo baixado: pode
            ("D:\\cache\\audioseal\\x.bin", 4 << 20), // desconhecido no cache feito à mão: fica
            ("D:\\Eduardo\\Cache\\.cache\\codex-runtimes\\r\\a.exe", 4 << 20), // peça do Codex: fica
            ("D:\\Eduardo\\Cache\\.cache\\huggingface\\h\\b.bin", 10 << 20),
            ("D:\\Applio\\logs\\Edu\\cache\\Edu.pth", 50 << 20), // modelo de voz num cache: nunca
            ("D:\\x\\pip\\cache\\w\\distutils-precedence.pth", 2 << 10), // .pth de pacote Python: não trava
            ("D:\\x\\pip\\cache\\w\\pkg.whl", 2 << 20),
            ("C:\\Users\\Edu\\AppData\\Roaming\\discord\\Cache\\data_1", 1 << 20),
            ("C:\\Windows\\ServiceProfiles\\Cache\\z", 9 << 20), // genérica dentro do Windows: fica fora
            ("C:\\Users\\Edu\\Fotos\\f.jpg", 9 << 20),
            ("C:\\Users\\Edu\\AppData\\Local\\Temp\\mini.txt", 10), // menos de 1 MB
        ]);
        let got: Vec<(String, u64)> = junk(&e).into_iter().map(|i| (i.path.display().to_string(), i.bytes >> 20)).collect();
        assert_eq!(
            got,
            vec![
                ("D:\\cache\\hf".to_string(), 11),
                ("D:\\Eduardo\\Cache\\.cache\\huggingface".to_string(), 10),
                ("C:\\Users\\Edu\\Cache\\.nuget\\packages\\p.nupkg".to_string(), 6),
                ("C:\\proj\\node_modules".to_string(), 5),
                ("C:\\Users\\Edu\\AppData\\Local\\Temp\\a".to_string(), 4),
                ("C:\\Users\\Edu\\AppData\\Local\\Temp\\b.tmp".to_string(), 3),
                ("D:\\x\\pip\\cache\\w".to_string(), 2),
                ("C:\\Users\\Edu\\AppData\\Roaming\\discord\\Cache".to_string(), 1),
            ]
        );
    }
    use super::*;
    fn idx(list: &[(&str, u64)]) -> Vec<Entry> {
        let mut v: Vec<Entry> = list.iter().map(|(p, n)| Entry::new(PathBuf::from(p), *n)).collect();
        v.sort_by(|a, b| a.searchable.cmp(&b.searchable));
        v
    }
    #[test]
    fn folder_sizes_and_children_with_tricky_names() {
        let e = idx(&[
            ("C:\\a\\x\\1.bin", 10),
            ("C:\\a\\x\\y\\2.bin", 20),
            ("C:\\a b\\3.bin", 40),
            ("C:\\a_\\4.bin", 80),
            ("C:\\a.txt", 160),
            ("C:\\Á\\5.bin", 320),
            ("C:\\a\\top.bin", 1),
        ]);
        let s = Sizes::new(&e);
        let (lo, hi) = range(&e, &prefix_of(Path::new("C:\\a")));
        // "a b", "a_" e "a.txt" não entram. "Á" e "a" são iguais depois de normalizar: somam juntas.
        assert_eq!(s.sum(lo, hi), 351);
        let root = children(&e, &s, Path::new("C:\\"));
        let got: Vec<(&str, bool, u64)> = root.iter().map(|c| (c.name.as_str(), c.is_dir, c.bytes)).collect();
        assert_eq!(got, vec![("Á", true, 351), ("a.txt", false, 160), ("a_", true, 80), ("a b", true, 40)]);
        let a = children(&e, &s, Path::new("C:\\a"));
        let got: Vec<(&str, u64, usize)> = a.iter().map(|c| (c.name.as_str(), c.bytes, c.files)).collect();
        assert_eq!(got, vec![("5.bin", 320, 1), ("x", 30, 2), ("top.bin", 1, 1)]);
        assert_eq!(a[1].path, PathBuf::from("C:\\a\\x"));
    }
    #[test]
    fn treemap_fills_the_rect_with_proportional_areas() {
        let sizes = [600, 300, 60, 30, 10];
        let r = squarify(&sizes, (0., 0., 100., 50.));
        let total: f32 = r.iter().map(|t| t.2 * t.3).sum();
        assert!((total - 5000.).abs() < 1.);
        for (t, &s) in r.iter().zip(&sizes) {
            assert!((t.2 * t.3 - s as f32 * 5.).abs() < 1., "{t:?} {s}");
            assert!(t.0 >= -0.01 && t.1 >= -0.01 && t.0 + t.2 <= 100.01 && t.1 + t.3 <= 50.01);
        }
    }
}
