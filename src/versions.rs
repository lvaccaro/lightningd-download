#[cfg(feature = "23_05_2")]
pub const VERSION: &str = "v23.05.2";

#[cfg(all(feature = "23_05", not(feature = "23_05_2")))]
pub const VERSION: &str = "v23.05";