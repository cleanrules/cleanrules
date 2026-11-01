//! Dates and datetimes (`spec/data-model.md`, *Datetimes*).

use std::cmp::Ordering;
use std::fmt;

use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use jiff::tz::{Offset, TimeZone};

/// Why a date or datetime is out of the model's range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DateTimeError {
    /// The year (of the UTC instant, for an aware datetime) is outside 1 to 9999.
    #[error("the year is outside 1 to 9999")]
    OutOfRange,
    /// The value is finer than a microsecond.
    #[error("the value is finer than a microsecond")]
    TooPrecise,
    /// The offset is not whole seconds within ±18 hours.
    #[error("the offset is not whole seconds within 18 hours")]
    BadOffset,
}

/// Check that a date is within years 1 to 9999.
///
/// # Errors
///
/// [`DateTimeError::OutOfRange`] otherwise.
pub fn check_date(date: Date) -> Result<Date, DateTimeError> {
    if (1..=9999).contains(&date.year()) {
        Ok(date)
    } else {
        Err(DateTimeError::OutOfRange)
    }
}

/// Check that a naive datetime is within range and to the microsecond.
///
/// # Errors
///
/// [`DateTimeError`] when it is not.
pub fn check_naive(datetime: DateTime) -> Result<DateTime, DateTimeError> {
    check_date(datetime.date())?;
    if datetime.subsec_nanosecond() % 1000 != 0 {
        return Err(DateTimeError::TooPrecise);
    }
    Ok(datetime)
}

/// An instant with a fixed UTC offset or an IANA time zone, to the microsecond.
///
/// Equality and ordering are by instant: the same instant in two zones is equal.
#[derive(Clone, Debug)]
pub struct AwareDateTime {
    timestamp: Timestamp,
    zone: Zone,
}

/// The zone of an aware datetime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Zone {
    /// A fixed offset from UTC; zero is written `Z`.
    Fixed(Offset),
    /// An IANA zone, by its name as written (aliases are not resolved).
    Named(String),
}

impl AwareDateTime {
    /// Build an aware datetime from an instant and a zone.
    ///
    /// # Errors
    ///
    /// [`DateTimeError`] when the instant's UTC year is outside 1 to 9999, the
    /// instant is finer than a microsecond, or a fixed offset is out of range.
    pub fn new(timestamp: Timestamp, zone: Zone) -> Result<Self, DateTimeError> {
        if timestamp.subsec_nanosecond() % 1000 != 0 {
            return Err(DateTimeError::TooPrecise);
        }
        check_date(timestamp.to_zoned(TimeZone::UTC).date())?;
        if let Zone::Fixed(offset) = &zone
            && offset.seconds().unsigned_abs() > 18 * 3600
        {
            return Err(DateTimeError::BadOffset);
        }
        Ok(Self { timestamp, zone })
    }

    /// Build an aware datetime from a native one: its instant, its offset at that
    /// instant, and its zone's IANA name when it has one (`spec/data-model.md`,
    /// *Time zones*). The zone is kept when the bundled database knows the name
    /// and gives it the same offset at that instant; otherwise the value keeps
    /// the native offset as a fixed offset.
    ///
    /// # Errors
    ///
    /// As [`AwareDateTime::new`].
    pub fn from_native(
        timestamp: Timestamp,
        offset: Offset,
        name: Option<&str>,
    ) -> Result<Self, DateTimeError> {
        let known = name.filter(|name| {
            jiff::tz::db()
                .get(name)
                .is_ok_and(|zone| zone.to_offset(timestamp) == offset)
        });
        let zone = match known {
            Some(name) => Zone::Named(name.to_owned()),
            None => Zone::Fixed(offset),
        };
        Self::new(timestamp, zone)
    }

    /// The instant.
    #[must_use]
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// The zone.
    #[must_use]
    pub fn zone(&self) -> &Zone {
        &self.zone
    }
}

impl PartialEq for AwareDateTime {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp
    }
}

impl Eq for AwareDateTime {}

impl PartialOrd for AwareDateTime {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AwareDateTime {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp.cmp(&other.timestamp)
    }
}

impl fmt::Display for AwareDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:?})", self.timestamp, self.zone)
    }
}

#[cfg(test)]
mod tests {
    use jiff::Timestamp;
    use jiff::civil::date;
    use jiff::tz::Offset;

    use super::{AwareDateTime, DateTimeError, Zone, check_date, check_naive};

    #[test]
    fn ranges() {
        assert!(check_date(date(1, 1, 1)).is_ok());
        assert!(check_date(date(9999, 12, 31)).is_ok());
        assert_eq!(check_date(date(0, 12, 31)), Err(DateTimeError::OutOfRange));
        assert_eq!(
            check_naive(date(2026, 1, 1).at(9, 30, 0, 1)),
            Err(DateTimeError::TooPrecise)
        );
        assert!(check_naive(date(2026, 1, 1).at(9, 30, 0, 1000)).is_ok());
    }

    #[test]
    fn aware_by_instant() {
        let instant: Timestamp = "2026-10-31T08:30:00Z".parse().unwrap();
        let utc = AwareDateTime::new(instant, Zone::Fixed(Offset::UTC)).unwrap();
        let madrid = AwareDateTime::new(instant, Zone::Named("Europe/Madrid".into())).unwrap();
        assert_eq!(utc, madrid);
        let before: Timestamp = "0000-12-31T23:30:00Z".parse().unwrap();
        assert_eq!(
            AwareDateTime::new(before, Zone::Fixed(Offset::UTC)),
            Err(DateTimeError::OutOfRange)
        );
        let offset = Offset::from_seconds(18 * 3600 + 1).unwrap();
        assert_eq!(
            AwareDateTime::new(instant, Zone::Fixed(offset)),
            Err(DateTimeError::BadOffset)
        );
    }

    #[test]
    fn native_zones_are_kept_only_when_they_agree() {
        // Summer time in Madrid ended on 2026-10-25, so the offset is +01:00.
        let instant: Timestamp = "2026-10-31T08:30:00Z".parse().unwrap();
        let one = Offset::from_seconds(3600).unwrap();
        let two = Offset::from_seconds(7200).unwrap();
        let zone = |name, offset| {
            AwareDateTime::from_native(instant, offset, name)
                .unwrap()
                .zone()
                .clone()
        };
        assert_eq!(
            zone(Some("Europe/Madrid"), one),
            Zone::Named("Europe/Madrid".into())
        );
        // The database gives another offset, the name is unknown, or there is none.
        assert_eq!(zone(Some("Europe/Madrid"), two), Zone::Fixed(two));
        assert_eq!(zone(Some("Mars/Olympus"), one), Zone::Fixed(one));
        assert_eq!(zone(None, Offset::UTC), Zone::Fixed(Offset::UTC));
    }
}
