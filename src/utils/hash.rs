use sha2::Digest;


pub fn hash_raw_code(raw_code_str: &str) -> String {
    format!("{:x}", sha2::Sha256::digest(raw_code_str.as_bytes()))
}