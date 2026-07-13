//! Shared Ruby version fixtures for tests.

/// Versions that satisfy `is_regular_release`.
pub const GOOD_VERSIONS: &[&str] = &[
    "3.3.0", "3.3.12", "3.2.0", "3.2.11", "3.2.2", "3.1.0", "3.1.12",
];

/// Versions that fail `is_regular_release`.
pub const BAD_VERSIONS: &[&str] = &[
    "2.7.0",
    "3.2.0-preview1",
    "3.2.0-rc2",
    "3.1.5-something",
    "3.0.5",
    "3.0.16",
];
