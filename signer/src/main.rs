// CI signing tool: `signer <file>` prints the base64 ed25519 signature of the
// file's bytes, using the key in $FREEPORT_UPDATER_KEY (base64 32-byte seed).
// The app verifies with the matching public key (core::update::PUBKEY_B64).

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signer, SigningKey};

fn main() {
    let path = std::env::args().nth(1).expect("uso: signer <archivo>");
    let key = std::env::var("FREEPORT_UPDATER_KEY").expect("falta FREEPORT_UPDATER_KEY");
    let seed = B64.decode(key.trim()).expect("clave base64 inválida");
    let seed: [u8; 32] = seed.as_slice().try_into().expect("la clave debe ser de 32 bytes");
    let sk = SigningKey::from_bytes(&seed);
    let bytes = std::fs::read(&path).expect("no se pudo leer el archivo");
    let sig = sk.sign(&bytes);
    println!("{}", B64.encode(sig.to_bytes()));
}
