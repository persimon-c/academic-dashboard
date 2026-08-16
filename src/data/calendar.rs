use chrono::NaiveDate;

pub const SEMESTER_START_DATE: &str = "2026-08-03";

pub fn date_to_canonical_week(date: NaiveDate) -> Option<u32> {
    let start_date = NaiveDate::parse_from_str(SEMESTER_START_DATE, "%Y-%m-%d").unwrap();
    if date < start_date {
        return None;
    }

    let days_diff = (date - start_date).num_days();
    let week = (days_diff / 7) + 1;

    if week <= 18 {
        Some(week as u32)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_to_canonical_week() {
        // Week 1
        assert_eq!(date_to_canonical_week(NaiveDate::from_ymd_opt(2026, 8, 3).unwrap()), Some(1));
        assert_eq!(date_to_canonical_week(NaiveDate::from_ymd_opt(2026, 8, 9).unwrap()), Some(1));
        
        // Week 2
        assert_eq!(date_to_canonical_week(NaiveDate::from_ymd_opt(2026, 8, 10).unwrap()), Some(2));
        
        // Before semester
        assert_eq!(date_to_canonical_week(NaiveDate::from_ymd_opt(2026, 8, 2).unwrap()), None);
        
        // After week 18 (Dec 6, 2026)
        assert_eq!(date_to_canonical_week(NaiveDate::from_ymd_opt(2026, 12, 7).unwrap()), None);
    }
}
