//! Histórico do disco: cada arquivo que entra, sai ou muda de tamanho fica anotado, para sempre.
//! Um arquivo por dia (`historico\AAAA-MM-DD.seekh`), só acrescentando linhas `quando|antes|depois|caminho`
//! (`-` = não existia). Os dias fechados viram `.seekh.gz` e ganham uma linha em `resumo.csv`.
//! `discos.csv` guarda o uso de cada disco, amostrado de hora em hora.
use crate::Entry;
use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufRead, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub struct Delta {
    pub at: u64,
    pub old: Option<u64>, // None = não existia
    pub new: Option<u64>, // None = sumiu
    pub path: Box<str>,
}
impl Delta {
    /// Variação em bytes (positiva = ocupou mais).
    pub fn change(&self) -> i64 {
        self.new.unwrap_or(0) as i64 - self.old.unwrap_or(0) as i64
    }
    fn line(&self) -> String {
        let f = |v: Option<u64>| v.map_or("-".to_string(), |n| n.to_string());
        format!("{}|{}|{}|{}\n", self.at, f(self.old), f(self.new), self.path)
    }
    fn parse(line: &str) -> Option<Self> {
        let mut it = line.splitn(4, '|');
        let at = it.next()?.parse().ok()?;
        let f = |s: &str| if s == "-" { Some(None) } else { s.parse().ok().map(Some) };
        let old = f(it.next()?)?;
        let new = f(it.next()?)?;
        let path = it.next()?.trim_end_matches(['\r', '\n']);
        (!path.is_empty() && (old.is_some() || new.is_some())).then(|| Self { at, old, new, path: path.into() })
    }
}

/// Diferença entre dois índices ordenados por `searchable` (ex.: o salvo e uma releitura).
pub fn diff_sorted(old: &[Entry], new: &[Entry], at: u64) -> Vec<Delta> {
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < old.len() || j < new.len() {
        let ord = match (old.get(i), new.get(j)) {
            (Some(a), Some(b)) => a.searchable.cmp(&b.searchable),
            (Some(_), None) => std::cmp::Ordering::Less,
            _ => std::cmp::Ordering::Greater,
        };
        match ord {
            std::cmp::Ordering::Less => {
                out.push(Delta { at, old: Some(old[i].bytes), new: None, path: old[i].path_str().into() });
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                out.push(Delta { at, old: None, new: Some(new[j].bytes), path: new[j].path_str().into() });
                j += 1;
            }
            std::cmp::Ordering::Equal => {
                if old[i].bytes != new[j].bytes {
                    out.push(Delta { at, old: Some(old[i].bytes), new: Some(new[j].bytes), path: new[j].path_str().into() });
                }
                i += 1;
                j += 1;
            }
        }
    }
    out
}

/// "2026-10-29" (dia local) de um horário.
pub fn day_name(secs: u64) -> String {
    let (y, m, d) = crate::clock::civil_from_days((secs as i64 + crate::clock::local_offset_secs()).div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}
/// Início (UTC) do dia local "AAAA-MM-DD".
pub fn day_start(day: &str) -> Option<u64> {
    crate::clock::date_range(day).ok().map(|r| r.0)
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct DaySum {
    pub grew: u64,   // bytes que entraram (arquivos novos e crescimentos)
    pub shrank: u64, // bytes que saíram
    pub events: usize,
}
impl DaySum {
    fn add(&mut self, d: &Delta) {
        let c = d.change();
        if c > 0 {
            self.grew += c as u64;
        } else {
            self.shrank += (-c) as u64;
        }
        self.events += 1;
    }
}

pub struct Log {
    pub dir: PathBuf,
}
impl Log {
    /// `historico` ao lado do programa.
    pub fn open() -> Option<Self> {
        Some(Self { dir: std::env::current_exe().ok()?.parent()?.join("historico") })
    }
    fn plain(&self, day: &str) -> PathBuf {
        self.dir.join(format!("{day}.seekh"))
    }
    fn packed(&self, day: &str) -> PathBuf {
        self.dir.join(format!("{day}.seekh.gz"))
    }
    pub fn record(&self, deltas: &[Delta]) -> io::Result<()> {
        if deltas.is_empty() {
            return Ok(());
        }
        fs::create_dir_all(&self.dir)?;
        let mut by_day: BTreeMap<String, Vec<&Delta>> = BTreeMap::new();
        for d in deltas {
            by_day.entry(day_name(d.at)).or_default().push(d);
        }
        for (day, list) in by_day {
            let mut w = BufWriter::new(fs::OpenOptions::new().create(true).append(true).open(self.plain(&day))?);
            for d in list {
                w.write_all(d.line().as_bytes())?;
            }
            w.flush()?;
        }
        Ok(())
    }
    /// Compacta os dias anteriores a `today` e anota o resumo de cada um.
    pub fn compact(&self, today: &str) -> io::Result<()> {
        let Ok(read) = fs::read_dir(&self.dir) else { return Ok(()) };
        let mut days: Vec<String> = read
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.strip_suffix(".seekh").map(str::to_owned))
            .filter(|d| d.as_str() < today)
            .collect();
        days.sort();
        for day in days {
            let plain = self.plain(&day);
            let data = fs::read(&plain)?;
            let mut sum = DaySum::default();
            for d in String::from_utf8_lossy(&data).lines().filter_map(Delta::parse) {
                sum.add(&d);
            }
            // Um .gz pode ter vários membros: acrescentar não estraga o que já está lá.
            let file = fs::OpenOptions::new().create(true).append(true).open(self.packed(&day))?;
            let mut gz = flate2::write::GzEncoder::new(file, flate2::Compression::best());
            gz.write_all(&data)?;
            gz.finish()?.sync_all()?;
            let mut resumo = fs::OpenOptions::new().create(true).append(true).open(self.dir.join("resumo.csv"))?;
            writeln!(resumo, "{day};{};{};{}", sum.grew, sum.shrank, sum.events)?;
            fs::remove_file(plain)?;
        }
        Ok(())
    }
    /// Tudo que mudou num dia.
    pub fn day(&self, day: &str) -> Vec<Delta> {
        let mut text = String::new();
        if let Ok(f) = fs::File::open(self.packed(day)) {
            let _ = flate2::read::MultiGzDecoder::new(f).read_to_string(&mut text);
        }
        if let Ok(f) = fs::File::open(self.plain(day)) {
            let _ = BufReader::new(f).read_to_string(&mut text);
        }
        text.lines().filter_map(Delta::parse).collect()
    }
    /// Resumo de cada dia registrado (o dia de hoje é calculado na hora).
    pub fn summary(&self) -> BTreeMap<String, DaySum> {
        let mut out: BTreeMap<String, DaySum> = BTreeMap::new();
        if let Ok(f) = fs::File::open(self.dir.join("resumo.csv")) {
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                let p: Vec<&str> = line.split(';').collect();
                if let [day, g, s, n] = p.as_slice()
                    && let (Ok(g), Ok(s), Ok(n)) = (g.parse::<u64>(), s.parse::<u64>(), n.parse::<usize>())
                {
                    let e = out.entry((*day).to_owned()).or_default();
                    e.grew += g;
                    e.shrank += s;
                    e.events += n;
                }
            }
        }
        if let Ok(read) = fs::read_dir(&self.dir) {
            for day in read.flatten().filter_map(|e| e.file_name().to_str()?.strip_suffix(".seekh").map(str::to_owned)) {
                let mut sum = DaySum::default();
                if let Ok(f) = fs::File::open(self.plain(&day)) {
                    for d in BufReader::new(f).lines().map_while(Result::ok).filter_map(|l| Delta::parse(&l)) {
                        sum.add(&d);
                    }
                }
                let e = out.entry(day).or_default();
                e.grew += sum.grew;
                e.shrank += sum.shrank;
                e.events += sum.events;
            }
        }
        out
    }
    /// Anota o uso dos discos: (disco, total, livre).
    pub fn sample_disks(&self, at: u64, drives: &[(PathBuf, u64, u64)]) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let mut f = fs::OpenOptions::new().create(true).append(true).open(self.dir.join("discos.csv"))?;
        for (d, total, free) in drives {
            writeln!(f, "{at};{};{total};{free}", d.display())?;
        }
        Ok(())
    }
    /// Amostras de uso: disco -> [(quando, usado)].
    pub fn disks(&self) -> BTreeMap<String, Vec<(u64, u64)>> {
        let mut out: BTreeMap<String, Vec<(u64, u64)>> = BTreeMap::new();
        if let Ok(f) = fs::File::open(self.dir.join("discos.csv")) {
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                let p: Vec<&str> = line.split(';').collect();
                if let [at, d, total, free] = p.as_slice()
                    && let (Ok(at), Ok(t), Ok(fr)) = (at.parse::<u64>(), total.parse::<u64>(), free.parse::<u64>())
                {
                    out.entry((*d).to_owned()).or_default().push((at, t.saturating_sub(fr)));
                }
            }
        }
        out
    }
    /// Espaço ocupado pelo próprio histórico.
    pub fn size(&self) -> u64 {
        fs::read_dir(&self.dir).map_or(0, |r| r.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum())
    }
}
/// Soma das variações dentro de `folder`, por item direto dela (subpasta com `\` no fim, ou arquivo):
/// (nome, variação em bytes, eventos), da maior variação para a menor.
pub fn by_folder(deltas: &[Delta], folder: &Path) -> Vec<(String, i64, usize)> {
    let prefix = crate::space::prefix_of(folder);
    let depth = prefix.matches('\\').count();
    let mut map: BTreeMap<String, (i64, usize)> = BTreeMap::new();
    for d in deltas {
        if !crate::normalize(&d.path).starts_with(&prefix) {
            continue;
        }
        let parts: Vec<&str> = d.path.split(['\\', '/']).collect();
        let name = if parts.len() > depth + 1 { format!("{}\\", parts[depth]) } else { parts[depth].to_owned() };
        let e = map.entry(name).or_default();
        e.0 += d.change();
        e.1 += 1;
    }
    let mut out: Vec<_> = map.into_iter().map(|(k, (c, n))| (k, c, n)).collect();
    out.sort_by(|a, b| b.1.abs().cmp(&a.1.abs()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(at: u64, old: Option<u64>, new: Option<u64>, p: &str) -> Delta {
        Delta { at, old, new, path: p.into() }
    }
    #[test]
    fn record_compact_and_read_back() {
        let dir = std::env::temp_dir().join(format!("seek-hist-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let log = Log { dir: dir.clone() };
        let t = day_start("2026-01-10").unwrap() + 3600;
        let a = vec![d(t, None, Some(100), "C:\\x\\a.bin"), d(t + 5, Some(100), Some(40), "C:\\x\\a.bin"), d(t + 9, Some(7), None, "C:\\y|z")];
        log.record(&a).unwrap();
        // linha cortada no meio (queda de energia) não estraga o resto
        fs::OpenOptions::new().append(true).open(log.plain("2026-01-10")).unwrap().write_all(b"123|4").unwrap();
        log.compact("2026-01-11").unwrap();
        assert!(!log.plain("2026-01-10").exists());
        let back = log.day("2026-01-10");
        assert_eq!(back[..2], a[..2]);
        assert_eq!(back[2].path.as_ref(), "C:\\y|z"); // o caminho é o último campo: pode ter qualquer coisa
        let today = vec![d(day_start("2026-01-11").unwrap() + 60, None, Some(5), "C:\\n")];
        log.record(&today).unwrap();
        let s = log.summary();
        assert_eq!(s["2026-01-10"], DaySum { grew: 100, shrank: 67, events: 3 });
        assert_eq!(s["2026-01-11"].grew, 5);
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn diff_finds_new_gone_and_resized() {
        let e = |p: &str, n| Entry::new(PathBuf::from(p), n);
        let old = vec![e("C:\\a", 1), e("C:\\b", 2), e("C:\\c", 3)];
        let new = vec![e("C:\\b", 2), e("C:\\c", 30), e("C:\\d", 4)];
        let got = diff_sorted(&old, &new, 9);
        assert_eq!(got, vec![d(9, Some(1), None, "C:\\a"), d(9, Some(3), Some(30), "C:\\c"), d(9, None, Some(4), "C:\\d")]);
        let by = by_folder(&got, Path::new("C:\\"));
        assert_eq!(by[0], ("c".to_string(), 27, 1));
    }
}
