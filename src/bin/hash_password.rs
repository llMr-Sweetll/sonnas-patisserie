//! Generates the argon2id hash for ADMIN_PASSWORD_HASH.
//! Usage: cargo run --bin hash-password -- 'your-admin-password'

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;

fn main() {
    let password = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: cargo run --bin hash-password -- '<password>'");
        std::process::exit(1);
    });
    if password.len() < 8 {
        eprintln!("pick a password of at least 8 characters");
        std::process::exit(1);
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("hashing failed")
        .to_string();
    println!("{hash}");
}
