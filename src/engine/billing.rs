use chrono::{Datelike, NaiveDate, NaiveTime, Utc};

/// Billing cycle tracker with configurable reset day.
pub struct BillingCycle {
    pub reset_day: u32,
}

impl BillingCycle {
    /// Returns the billing month string (e.g., "2026-03") for the given time.
    ///
    /// If the current day is before the reset day, we're still in the
    /// previous billing month.
    pub fn current_month(&self, now: chrono::DateTime<Utc>) -> String {
        let year = now.year();
        let month = now.month();
        let day = now.day();

        if day < self.reset_day {
            // Before reset day: still in previous billing month
            if month == 1 {
                format!("{}-12", year - 1)
            } else {
                format!("{}-{:02}", year, month - 1)
            }
        } else {
            format!("{}-{:02}", year, month)
        }
    }

    /// Returns true if the stored billing month differs from the current one.
    pub fn should_reset(&self, stored_month: &str, now: chrono::DateTime<Utc>) -> bool {
        stored_month != self.current_month(now)
    }

    /// Returns the number of days until the next billing reset.
    pub fn days_remaining(&self, now: chrono::DateTime<Utc>) -> i32 {
        let year = now.year();
        let month = now.month();
        let day = now.day();

        let next_reset = if day < self.reset_day {
            NaiveDate::from_ymd_opt(year, month, self.reset_day).unwrap()
        } else {
            let (next_year, next_month) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
            NaiveDate::from_ymd_opt(next_year, next_month, self.reset_day).unwrap()
        };

        let now_date = now.date_naive();
        let next_reset_dt = next_reset
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
            .and_utc();
        let now_dt = now_date
            .and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
            .and_utc();
        let diff = next_reset_dt.signed_duration_since(now_dt);
        (diff.num_hours() / 24) as i32 + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn billing_current_month() {
        let bc = BillingCycle { reset_day: 15 };

        let tests = [
            ("after reset day", Utc.with_ymd_and_hms(2026, 3, 20, 0, 0, 0).unwrap(), "2026-03"),
            ("on reset day", Utc.with_ymd_and_hms(2026, 3, 15, 0, 0, 0).unwrap(), "2026-03"),
            ("before reset day", Utc.with_ymd_and_hms(2026, 3, 10, 0, 0, 0).unwrap(), "2026-02"),
            ("jan before reset", Utc.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap(), "2025-12"),
            ("dec after reset", Utc.with_ymd_and_hms(2025, 12, 20, 0, 0, 0).unwrap(), "2025-12"),
        ];

        for (name, date, want) in &tests {
            let got = bc.current_month(*date);
            assert_eq!(&got, *want, "{name}: CurrentMonth({date}) = {got}, want {want}");
        }
    }

    #[test]
    fn billing_reset_day_1() {
        let bc = BillingCycle { reset_day: 1 };

        // On the 1st, should be current month
        let got = bc.current_month(Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap());
        assert_eq!(got, "2026-03");

        // On the 28th, should still be current month
        let got = bc.current_month(Utc.with_ymd_and_hms(2026, 3, 28, 0, 0, 0).unwrap());
        assert_eq!(got, "2026-03");
    }

    #[test]
    fn billing_should_reset() {
        let bc = BillingCycle { reset_day: 1 };

        let now = Utc.with_ymd_and_hms(2026, 4, 5, 0, 0, 0).unwrap();
        assert!(bc.should_reset("2026-03", now), "should reset when month changed");
        assert!(!bc.should_reset("2026-04", now), "should not reset when month is current");
    }

    #[test]
    fn billing_days_remaining() {
        let bc = BillingCycle { reset_day: 15 };

        // 10 days before reset
        let days = bc.days_remaining(Utc.with_ymd_and_hms(2026, 3, 5, 0, 0, 0).unwrap());
        assert!(days >= 10 && days <= 11, "days remaining = {days}, want ~10");

        // After reset day, next month
        let days = bc.days_remaining(Utc.with_ymd_and_hms(2026, 3, 20, 0, 0, 0).unwrap());
        assert!(days >= 25 && days <= 27, "days remaining after reset = {days}, want ~26");
    }
}
