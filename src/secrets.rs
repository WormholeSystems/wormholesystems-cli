use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::Rng;
use rand::distr::Alphanumeric;

/// Alphanumeric only, so values stay safe unquoted in .env files and MySQL.
pub fn alphanumeric(len: usize) -> String {
    rand::rng()
        .sample_iter(Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

pub fn hex(len: usize) -> String {
    let mut rng = rand::rng();
    (0..len)
        .map(|_| char::from_digit(rng.random_range(0..16), 16).unwrap())
        .collect()
}

/// Same format as `php artisan key:generate`: base64 of 32 random bytes.
pub fn laravel_app_key() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    format!("base64:{}", STANDARD.encode(bytes))
}
