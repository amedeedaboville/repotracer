use chrono::{
    format::ParseErrorKind, DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone,
    Utc,
};
use gix::date::{time::Sign, Time};
use indicatif::{ProgressBar, ProgressStyle};

pub fn pb_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}")
        .expect("error with progress bar style")
}
pub fn pb_default(total: usize) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(pb_style());
    pb
}

pub fn parse_loose_datetime(input: &str) -> Result<DateTime<Utc>, ParseErrorKind> {
    // Full date, time, and timezone
    if let Ok(dt) = DateTime::parse_from_str(input, "%Y-%m-%d %H:%M:%S %z") {
        return Ok(dt.with_timezone(&Utc));
    }

    // Date and time without timezone (assume UTC)
    if let Ok(ndt) = NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M:%S") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(ndt, Utc));
    }

    // Only date
    if let Ok(nd) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(
            nd.and_hms_opt(0, 0, 0).unwrap(),
            Utc,
        ));
    }

    // Only time (assume today's date)
    if let Ok(nt) = NaiveTime::parse_from_str(input, "%H:%M:%S") {
        let today = Utc::now().date_naive();
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(
            today.and_time(nt),
            Utc,
        ));
    }

    Err(ParseErrorKind::NotEnough)
}

// ai generated, lol
pub fn gix_time_to_chrono(gix_time: Time) -> DateTime<Utc> {
    let naive = NaiveDateTime::from_timestamp_opt(gix_time.seconds, 0).expect("Invalid timestamp");

    let offset_sign = match gix_time.sign {
        Sign::Plus => 1,
        Sign::Minus => -1,
    };
    let offset = FixedOffset::east_opt(offset_sign * gix_time.offset).expect("Invalid offset");
    let datetime_with_offset = offset.from_utc_datetime(&naive);
    datetime_with_offset.with_timezone(&Utc)
}
