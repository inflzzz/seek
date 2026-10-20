//! Datas: horário local do Windows, formatação e conversão de datas como `2023-05-14`.

pub fn now_secs() -> u64 {
    crate::unix_secs(std::time::SystemTime::now())
}
/// Diferença entre o horário local e o UTC, em segundos (inclui o horário de verão atual).
pub fn local_offset_secs() -> i64 {
    static OFFSET: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *OFFSET.get_or_init(|| {
        use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        let mut t: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
        let id = unsafe { GetTimeZoneInformation(&mut t) };
        let bias = t.Bias + if id == 2 { t.DaylightBias } else { t.StandardBias };
        -(bias as i64) * 60
    })
}
/// Dias desde 1970 para uma data do calendário.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
/// Data do calendário (ano, mês, dia) para dias desde 1970.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}
/// "20/10/2026 00:16" no horário local; "—" se a data for desconhecida (0).
pub fn format_local(secs: u64) -> String {
    if secs == 0 {
        return "—".into();
    }
    let t = secs as i64 + local_offset_secs();
    let (y, m, d) = civil_from_days(t.div_euclid(86_400));
    let sod = t.rem_euclid(86_400);
    format!("{d:02}/{m:02}/{y} {:02}:{:02}", sod / 3600, sod % 3600 / 60)
}
/// Início (em UTC) do dia local que contém `secs`.
pub fn local_day_start(secs: u64) -> u64 {
    let off = local_offset_secs();
    let local = secs as i64 + off;
    (local - local.rem_euclid(86_400) - off).max(0) as u64
}
fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}
/// `2023`, `2023-05` ou `2023-05-14` -> [início, fim) em UTC, no horário local.
fn period(s: &str) -> Option<(u64, u64)> {
    let mut parts = s.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: Option<u32> = parts.next().map(str::parse).transpose().ok()?;
    let d: Option<u32> = parts.next().map(str::parse).transpose().ok()?;
    if parts.next().is_some() || !(1900..=2200).contains(&y) {
        return None;
    }
    let (m0, d0) = (m.unwrap_or(1), d.unwrap_or(1));
    if !(1..=12).contains(&m0) || d0 < 1 || d0 > days_in_month(y, m0) {
        return None;
    }
    let start = days_from_civil(y, m0, d0);
    let end = match (m, d) {
        (Some(_), Some(_)) => start + 1,
        (Some(m), None) => days_from_civil(y + i64::from(m == 12), if m == 12 { 1 } else { m + 1 }, 1),
        _ => days_from_civil(y + 1, 1, 1),
    };
    let off = local_offset_secs();
    Some(((start * 86_400 - off).max(0) as u64, (end * 86_400 - off).max(0) as u64))
}
/// Entende o valor de `modificado:` e `criado:` e devolve o intervalo [início, fim) em UTC.
/// Aceita `hoje`, `ontem`, `semana`, `mes`, `ano`, `2023`, `2023-05`, `2023-05-14`,
/// intervalos (`2023-01..2023-06`) e comparações (`>2023-01-01`, `<2023`).
pub fn date_range(value: &str) -> Result<(u64, u64), String> {
    let bad = || format!("Data inválida: {value}. Use hoje, ontem, semana, 2023, 2023-05 ou 2023-05-14.");
    let (now, today) = (now_secs(), local_day_start(now_secs()));
    let day = 86_400;
    match value {
        "hoje" => return Ok((today, today + day)),
        "ontem" => return Ok((today.saturating_sub(day), today)),
        "semana" => return Ok((now.saturating_sub(7 * day), now + 1)),
        "mes" => return Ok((now.saturating_sub(30 * day), now + 1)),
        "ano" => return Ok((now.saturating_sub(365 * day), now + 1)),
        _ => {}
    }
    if let Some((a, b)) = value.split_once("..") {
        return Ok((period(a).ok_or_else(bad)?.0, period(b).ok_or_else(bad)?.1));
    }
    if let Some(x) = value.strip_prefix('>') {
        return Ok((period(x).ok_or_else(bad)?.1, u64::MAX));
    }
    if let Some(x) = value.strip_prefix('<') {
        return Ok((0, period(x).ok_or_else(bad)?.0));
    }
    period(value).ok_or_else(bad)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calendar_roundtrip() {
        for days in [-1, 0, 19_000, 20_380, 30_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
    }
    #[test]
    fn date_ranges() {
        let (lo, hi) = date_range("2023").unwrap();
        assert!(hi - lo >= 365 * 86_400 - 7200 && hi - lo <= 366 * 86_400 + 7200);
        let (lo, hi) = date_range("2023-05-14").unwrap();
        assert!((hi - lo) / 3600 >= 23 && (hi - lo) / 3600 <= 25);
        let (a, b) = date_range("2023-01..2023-03").unwrap();
        assert!(b > a + 80 * 86_400);
        assert!(date_range(">2023").unwrap().0 > date_range("2023").unwrap().0);
        assert_eq!(date_range("<2023").unwrap().1, date_range("2023").unwrap().0);
        assert!(date_range("2023-02-30").is_err());
        assert!(date_range("lixo").is_err());
        let (lo, hi) = date_range("hoje").unwrap();
        assert!(lo <= now_secs() && now_secs() < hi);
    }
}
