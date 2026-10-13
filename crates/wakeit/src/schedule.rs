use crate::config::{Schedule, duration};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, LocalResult, TimeZone, Utc};
use chrono_tz::Tz;
use croner::Cron;

pub fn validate(s: &Schedule) -> Result<()> {
    ensure!(
        [s.at.is_some(), s.every.is_some(), s.cron.is_some()]
            .iter()
            .filter(|v| **v)
            .count()
            == 1,
        "schedule requires exactly one of at/every/cron"
    );
    let _ = timezone(s)?;
    if let Some(e) = &s.every {
        duration(e)?;
    }
    if let Some(c) = &s.cron {
        ensure!(
            c.split_whitespace().count() == 5,
            "cron requires five fields"
        );
        let _: Cron = c.parse()?;
    }
    Ok(())
}
pub fn next(s: &Schedule, after: DateTime<Utc>, inclusive: bool) -> Result<Option<DateTime<Utc>>> {
    validate(s)?;
    if let Some(at) = s.at {
        return Ok((at > after || (inclusive && at == after)).then_some(at));
    }
    if let Some(e) = &s.every {
        return Ok(Some(after + chrono::Duration::from_std(duration(e)?)?));
    }
    let tz = timezone(s)?;
    let cron: Cron = s.cron.as_ref().context("cron")?.parse()?;
    let mut cursor = after.with_timezone(&tz);
    let mut include = inclusive;
    // During a fall-back fold only the earlier occurrence belongs to this schedule.
    for _ in 0..180 {
        let n = cron.find_next_occurrence(&cursor, include)?;
        let accepted = cron.is_time_matching(&n)?
            && match tz.from_local_datetime(&n.naive_local()) {
                LocalResult::Ambiguous(a, b) => n == a.min(b),
                LocalResult::Single(_) => true,
                LocalResult::None => false,
            };
        if accepted {
            return Ok(Some(n.with_timezone(&Utc)));
        }
        cursor = n;
        include = false;
    }
    anyhow::bail!("could not resolve cron occurrence")
}
pub fn previous_cron(s: &Schedule, at: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let tz = timezone(s)?;
    let cron: Cron = s.cron.as_ref().context("cron")?.parse()?;
    let local = at.with_timezone(&tz);
    let mut cursor = local;
    if let LocalResult::Ambiguous(a, b) = tz.from_local_datetime(&local.naive_local())
        && local == a.max(b)
    {
        cursor = (at + chrono::Duration::seconds((a - b).num_seconds().abs())).with_timezone(&tz);
    }
    for _ in 0..180 {
        let n = cron.find_previous_occurrence(&cursor, true)?;
        let candidate = match tz.from_local_datetime(&n.naive_local()) {
            LocalResult::Ambiguous(a, b) => a.min(b),
            _ => n,
        };
        if candidate.with_timezone(&Utc) <= at && cron.is_time_matching(&candidate)? {
            return Ok(candidate.with_timezone(&Utc));
        }
        cursor = n - chrono::Duration::minutes(1);
    }
    anyhow::bail!("could not resolve previous cron occurrence")
}
pub fn latest_interval(
    first: DateTime<Utc>,
    now: DateTime<Utc>,
    interval: &str,
) -> Result<DateTime<Utc>> {
    let step = i64::try_from(duration(interval)?.as_nanos())?;
    let elapsed = (now - first)
        .num_nanoseconds()
        .context("interval age exceeds supported range")?
        .max(0);
    Ok(first + chrono::Duration::nanoseconds(elapsed / step * step))
}
fn timezone(s: &Schedule) -> Result<Tz> {
    Ok(if s.timezone.is_empty() {
        chrono_tz::Asia::Shanghai
    } else {
        s.timezone.parse().context("unknown IANA timezone")?
    })
}
pub fn preview(s: &Schedule, after: DateTime<Utc>) -> Result<Vec<DateTime<Utc>>> {
    let mut out = vec![];
    let mut cursor = after;
    for _ in 0..5 {
        match next(s, cursor, false)? {
            Some(n) => {
                out.push(n);
                cursor = n
            }
            None => break,
        }
    }
    Ok(out)
}
