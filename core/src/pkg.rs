//! Reader for PS4 "fake" PKG packages (fPKG): the format homebrew tools write
//! for dumped games and patches. Extracts the `sce_sys` entries and the PFS
//! image into a plain game folder (`eboot.bin`, `sce_module/`, `sce_sys/`…),
//! which is what the PS4 native ports (bbport, Knack II, Uncharted…) consume.
//!
//! Layout (big-endian header): an entry table (param.sfo, icons, keys…) and a
//! PFS image encrypted with AES-XTS. The data key comes from the public fPKG
//! key set: RSA(PKCS#1 v1.5) → dk3 → AES-CBC(image key) → RSA → ekpfs →
//! HMAC-SHA256(ekpfs, seed) → tweak/data keys. Inside, a PFSC stream of
//! 64 KiB blocks (zlib or raw) holds the inner PFS: superblock, inodes, dirs.

use crate::error::{AppError, AppResult};
use crate::pkg_keys as keys;
use aes::cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt, KeyInit};
use aes::Aes128;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const PKG_MAGIC: u32 = 0x7F43_4E54;
const BLOCK: usize = 0x10000; // PFSC decompressed block
const SECTOR: u64 = 0x1000; // XTS sector
const INODE_SIZE: usize = 0xA8;
const INODES_PER_BLOCK: usize = BLOCK / INODE_SIZE;

/// What a package is, read from its header and `param.sfo`.
#[derive(Debug, Clone)]
pub struct PkgInfo {
    pub title_id: String,
    pub content_id: String,
    /// "gd" = game, "gp" = patch, "ac" = add-on (from param.sfo CATEGORY).
    pub category: String,
    pub title: String,
    pub app_ver: String,
    pub version: String,
    pub size: u64,
}

impl PkgInfo {
    pub fn is_patch(&self) -> bool {
        self.category.starts_with("gp")
    }
    pub fn is_dlc(&self) -> bool {
        self.category.starts_with("ac")
    }
}

#[derive(Debug, Clone, Copy)]
struct Header {
    table_entry_count: u32,
    table_entry_offset: u32,
    content_offset: u64,
    content_size: u64,
    pfs_image_offset: u64,
    pfs_image_size: u64,
    pkg_size: u64,
    pfs_cache_size: u64,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    id: u32,
    offset: u32,
    size: u32,
    raw: [u8; 32],
}

fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes(b[o..o + 4].try_into().unwrap())
}
fn be64(b: &[u8], o: usize) -> u64 {
    u64::from_be_bytes(b[o..o + 8].try_into().unwrap())
}
fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn le64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

fn read_at(f: &mut std::fs::File, off: u64, len: usize) -> AppResult<Vec<u8>> {
    let mut buf = vec![0u8; len];
    f.seek(SeekFrom::Start(off))?;
    f.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_header(f: &mut std::fs::File) -> AppResult<(Header, String)> {
    let h = read_at(f, 0, 0x1000).map_err(|_| AppError::msg("el archivo es demasiado pequeño para ser un PKG"))?;
    if be32(&h, 0) != PKG_MAGIC {
        return Err(AppError::msg("no es un PKG de PS4 (cabecera incorrecta)"));
    }
    let content_id = String::from_utf8_lossy(&h[0x40..0x64]).trim_end_matches('\0').to_string();
    Ok((
        Header {
            table_entry_count: be32(&h, 0x10),
            table_entry_offset: be32(&h, 0x18),
            content_offset: be64(&h, 0x30),
            content_size: be64(&h, 0x38),
            pfs_image_offset: be64(&h, 0x410),
            pfs_image_size: be64(&h, 0x418),
            pkg_size: be64(&h, 0x430),
            pfs_cache_size: be32(&h, 0x43C) as u64,
        },
        content_id,
    ))
}

fn read_entries(f: &mut std::fs::File, h: &Header) -> AppResult<Vec<Entry>> {
    let n = h.table_entry_count as usize;
    let raw = read_at(f, h.table_entry_offset as u64, n * 32)?;
    Ok((0..n)
        .map(|i| {
            let e = &raw[i * 32..i * 32 + 32];
            Entry {
                id: be32(e, 0),
                offset: be32(e, 16),
                size: be32(e, 20),
                raw: e.try_into().unwrap(),
            }
        })
        .collect())
}

/// Minimal `param.sfo` reader: string values by key.
fn parse_sfo(sfo: &[u8]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if sfo.len() < 0x14 || &sfo[0..4] != b"\0PSF" {
        return out;
    }
    let key_table = le32(sfo, 8) as usize;
    let data_table = le32(sfo, 0xC) as usize;
    let count = le32(sfo, 0x10) as usize;
    for i in 0..count {
        let ix = 0x14 + i * 16;
        if ix + 16 > sfo.len() {
            break;
        }
        let key_off = u16::from_le_bytes([sfo[ix], sfo[ix + 1]]) as usize;
        let fmt = u16::from_le_bytes([sfo[ix + 2], sfo[ix + 3]]);
        let len = le32(sfo, ix + 4) as usize;
        let data_off = le32(sfo, ix + 12) as usize;
        let key_end = sfo[key_table + key_off..].iter().position(|&c| c == 0).map(|p| key_table + key_off + p);
        let (Some(key_end), true) = (key_end, data_table + data_off + len <= sfo.len()) else { continue };
        let key = String::from_utf8_lossy(&sfo[key_table + key_off..key_end]).to_string();
        let data = &sfo[data_table + data_off..data_table + data_off + len];
        let val = match fmt {
            0x0404 if len >= 4 => le32(data, 0).to_string(),
            _ => String::from_utf8_lossy(data).trim_end_matches('\0').to_string(),
        };
        out.insert(key, val);
    }
    out
}

fn entry_name(id: u32) -> Option<&'static str> {
    keys::ENTRY_NAMES.iter().find(|(k, _)| *k == id).map(|(_, n)| *n)
}

/// Reads title id, category and versions without touching the encrypted image.
pub fn info(path: &Path) -> AppResult<PkgInfo> {
    let mut f = std::fs::File::open(path)?;
    let (h, content_id) = read_header(&mut f)?;
    let entries = read_entries(&mut f, &h)?;
    let sfo = entries
        .iter()
        .find(|e| e.id == 0x1000)
        .map(|e| read_at(&mut f, e.offset as u64, e.size as usize))
        .transpose()?
        .map(|b| parse_sfo(&b))
        .unwrap_or_default();
    let title_id = sfo
        .get("TITLE_ID")
        .cloned()
        .unwrap_or_else(|| content_id.get(7..16).unwrap_or("").to_string());
    Ok(PkgInfo {
        title_id,
        content_id,
        category: sfo.get("CATEGORY").cloned().unwrap_or_default(),
        title: sfo.get("TITLE").cloned().unwrap_or_default(),
        app_ver: sfo.get("APP_VER").cloned().unwrap_or_default(),
        version: sfo.get("VERSION").cloned().unwrap_or_default(),
        size: h.pkg_size,
    })
}

// ───────────────────────────── crypto ─────────────────────────────

fn rsa_key(n: &[u8], d: &[u8], p: &[u8], q: &[u8]) -> AppResult<rsa::RsaPrivateKey> {
    use rsa::BigUint;
    rsa::RsaPrivateKey::from_components(
        BigUint::from_bytes_be(n),
        BigUint::from(65537u32),
        BigUint::from_bytes_be(d),
        vec![BigUint::from_bytes_be(p), BigUint::from_bytes_be(q)],
    )
    .map_err(|e| AppError::msg(format!("clave RSA del PKG inválida: {e}")))
}

/// RSAES-PKCS1-v1_5 decryption; returns the first 32 bytes of the message.
fn rsa_decrypt32(key: &rsa::RsaPrivateKey, ct: &[u8]) -> AppResult<[u8; 32]> {
    let msg = key
        .decrypt(rsa::Pkcs1v15Encrypt, ct)
        .map_err(|e| AppError::msg(format!("no se pudo descifrar la clave del PKG ({e}); ¿es un fake PKG?")))?;
    if msg.len() < 32 {
        return Err(AppError::msg("clave del PKG demasiado corta"));
    }
    Ok(msg[..32].try_into().unwrap())
}

fn aes_cbc_decrypt(ivkey: &[u8; 32], data: &mut [u8]) {
    let cipher = Aes128::new(GenericArray::from_slice(&ivkey[16..32]));
    let mut prev: [u8; 16] = ivkey[0..16].try_into().unwrap();
    for chunk in data.chunks_exact_mut(16) {
        let c: [u8; 16] = chunk.try_into().unwrap();
        let mut b = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut b);
        for i in 0..16 {
            chunk[i] = b[i] ^ prev[i];
        }
        prev = c;
    }
}

struct Xts {
    data: Aes128,
    tweak: Aes128,
}

impl Xts {
    fn new(ekpfs: &[u8; 32], seed: &[u8; 16]) -> Self {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(ekpfs).expect("hmac key");
        mac.update(&1u32.to_le_bytes());
        mac.update(seed);
        let d = mac.finalize().into_bytes();
        Xts {
            tweak: Aes128::new(GenericArray::from_slice(&d[0..16])),
            data: Aes128::new(GenericArray::from_slice(&d[16..32])),
        }
    }

    /// Decrypts whole 4 KiB sectors in place; `first_sector` is the XTS
    /// sector number of `buf[0]` (image offset / 0x1000).
    fn decrypt(&self, buf: &mut [u8], first_sector: u64) {
        for (k, sector) in buf.chunks_exact_mut(SECTOR as usize).enumerate() {
            let mut t = [0u8; 16];
            t[..8].copy_from_slice(&(first_sector + k as u64).to_le_bytes());
            let mut tw = GenericArray::clone_from_slice(&t);
            self.tweak.encrypt_block(&mut tw);
            for blk in sector.chunks_exact_mut(16) {
                let mut x = GenericArray::clone_from_slice(blk);
                for i in 0..16 {
                    x[i] ^= tw[i];
                }
                self.data.decrypt_block(&mut x);
                for i in 0..16 {
                    blk[i] = x[i] ^ tw[i];
                }
                // multiply tweak by alpha in GF(2^128), little-endian bit order
                let mut carry = 0u8;
                for b in tw.iter_mut() {
                    let next = (*b >> 7) & 1;
                    *b = (*b << 1) | carry;
                    carry = next;
                }
                if carry != 0 {
                    tw[0] ^= 0x87;
                }
            }
        }
    }
}

// ───────────────────────────── PFS ─────────────────────────────

struct Pfs {
    file: std::fs::File,
    image_off: u64,
    image_size: u64,
    xts: Xts,
    pfsc_off: u64,
    sector_map: Vec<u64>,
    cache: HashMap<usize, Vec<u8>>, // decompressed metadata blocks
}

#[derive(Debug, Clone, Copy)]
struct Inode {
    mode: u16,
    size: u64,
    blocks: u32,
    loc: u32,
}

impl Pfs {
    /// Decrypts `len` bytes at `off` (relative to the image start).
    fn decrypt_range(&mut self, off: u64, len: usize) -> AppResult<Vec<u8>> {
        let start = off & !(SECTOR - 1);
        let end = (off + len as u64 + SECTOR - 1) & !(SECTOR - 1);
        let end = end.min(self.image_size);
        if start >= end {
            return Err(AppError::msg("lectura fuera de la imagen PFS"));
        }
        let mut buf = read_at(&mut self.file, self.image_off + start, (end - start) as usize)?;
        self.xts.decrypt(&mut buf, start / SECTOR);
        let skip = (off - start) as usize;
        let take = len.min(buf.len().saturating_sub(skip));
        Ok(buf[skip..skip + take].to_vec())
    }

    fn open(file: std::fs::File, h: &Header, ekpfs: &[u8; 32]) -> AppResult<Self> {
        let mut file = file;
        let seed: [u8; 16] = read_at(&mut file, h.pfs_image_offset + 0x370, 16)?.try_into().unwrap();
        let mut pfs = Pfs {
            file,
            image_off: h.pfs_image_offset,
            image_size: h.pfs_image_size,
            xts: Xts::new(ekpfs, &seed),
            pfsc_off: 0,
            sector_map: Vec::new(),
            cache: HashMap::new(),
        };
        // The PFSC stream starts at a 64 KiB boundary after the outer header.
        // It sits within the first `pfs_cache_size * 2` bytes (big games put it
        // tens of MB in), so scan that far at 64 KiB steps.
        let limit = (h.pfs_cache_size * 2).max(0x4000000).min(pfs.image_size);
        let mut found = None;
        let mut off = 0x20000u64;
        while off + 0x30 <= limit {
            let b = pfs.decrypt_range(off, 0x30)?;
            if &b[0..4] == b"PFSC" {
                found = Some((off, b));
                break;
            }
            off += 0x10000;
        }
        let (pfsc_off, hdr) = found.ok_or_else(|| AppError::msg("no se encontró el sistema de archivos PFSC (¿PKG cifrado con otra clave?)"))?;
        let block_sz2 = le64(&hdr, 0x10);
        let block_offsets = le64(&hdr, 0x18);
        let data_length = le64(&hdr, 0x28);
        if block_sz2 != BLOCK as u64 {
            return Err(AppError::msg(format!("tamaño de bloque PFSC no soportado: {block_sz2:#x}")));
        }
        let num_blocks = (data_length / block_sz2) as usize;
        let map = pfs.decrypt_range(pfsc_off + block_offsets, (num_blocks + 1) * 8)?;
        pfs.sector_map = (0..=num_blocks).map(|i| le64(&map, i * 8)).collect();
        pfs.pfsc_off = pfsc_off;
        Ok(pfs)
    }

    /// One 64 KiB block of the inner file system, decompressed.
    fn block(&mut self, i: usize) -> AppResult<Vec<u8>> {
        if i + 1 >= self.sector_map.len() {
            return Err(AppError::msg(format!("bloque PFS {i} fuera de rango")));
        }
        let (a, b) = (self.sector_map[i], self.sector_map[i + 1]);
        let comp = (b - a) as usize;
        let mut out = vec![0u8; BLOCK];
        if comp == 0 {
            return Ok(out);
        }
        let raw = self.decrypt_range(self.pfsc_off + a, comp)?;
        if comp >= BLOCK {
            out.copy_from_slice(&raw[..BLOCK]);
        } else {
            let mut z = flate2::read::ZlibDecoder::new(&raw[..]);
            let mut got = 0;
            while got < BLOCK {
                match z.read(&mut out[got..]) {
                    Ok(0) => break,
                    Ok(n) => got += n,
                    Err(e) => return Err(AppError::msg(format!("bloque PFS {i} corrupto: {e}"))),
                }
            }
        }
        Ok(out)
    }

    fn meta_block(&mut self, i: usize) -> AppResult<Vec<u8>> {
        if let Some(b) = self.cache.get(&i) {
            return Ok(b.clone());
        }
        let b = self.block(i)?;
        self.cache.insert(i, b.clone());
        Ok(b)
    }

    fn inode(&mut self, ino: u64) -> AppResult<Inode> {
        let blk = 1 + ino as usize / INODES_PER_BLOCK;
        let off = (ino as usize % INODES_PER_BLOCK) * INODE_SIZE;
        let b = self.meta_block(blk)?;
        let n = &b[off..off + INODE_SIZE];
        Ok(Inode {
            mode: u16::from_le_bytes([n[0], n[1]]),
            size: le64(n, 8),
            blocks: le32(n, 0x60),
            loc: le32(n, 0x64),
        })
    }

    /// Directory entries of a dir inode: (ino, type, name).
    fn dirents(&mut self, dir: &Inode) -> AppResult<Vec<(u64, u32, String)>> {
        let mut out = Vec::new();
        for j in 0..dir.blocks as usize {
            let b = self.meta_block(dir.loc as usize + j)?;
            let mut p = 0usize;
            while p + 16 <= BLOCK {
                let ino = le32(&b, p);
                let typ = le32(&b, p + 4);
                let namelen = le32(&b, p + 8) as usize;
                let entsize = le32(&b, p + 12) as usize;
                if ino == 0 || entsize == 0 || p + 16 + namelen > BLOCK {
                    break;
                }
                out.push((ino as u64, typ, String::from_utf8_lossy(&b[p + 16..p + 16 + namelen]).to_string()));
                p += entsize;
            }
        }
        Ok(out)
    }

    /// Recursively lists files under `dir` as (relative path, inode).
    fn walk(&mut self, dir: &Inode, prefix: &Path, out: &mut Vec<(PathBuf, Inode)>, depth: usize) -> AppResult<()> {
        if depth > 64 {
            return Err(AppError::msg("árbol PFS demasiado profundo"));
        }
        for (ino, typ, name) in self.dirents(dir)? {
            match typ {
                2 => {
                    if depth == 0 && name == "flat_path_table" {
                        continue; // superroot bookkeeping, not part of the game
                    }
                    out.push((prefix.join(&name), self.inode(ino)?))
                }
                3 => {
                    let sub = self.inode(ino)?;
                    // The superroot holds `uroot` (the real root) and a path table.
                    let next = if depth == 0 && name == "uroot" { prefix.to_path_buf() } else { prefix.join(&name) };
                    self.walk(&sub, &next, out, depth + 1)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn extract_file(&mut self, node: &Inode, dest: &Path, progress: &mut dyn FnMut(u64)) -> AppResult<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::io::BufWriter::new(std::fs::File::create(dest)?);
        let mut left = node.size;
        for j in 0..node.blocks as u64 {
            if left == 0 {
                break;
            }
            let b = self.block((node.loc as u64 + j) as usize)?;
            let n = left.min(BLOCK as u64) as usize;
            out.write_all(&b[..n])?;
            left -= n as u64;
            progress(n as u64);
        }
        out.flush()?;
        Ok(())
    }
}

// ───────────────────────────── extraction ─────────────────────────────

/// Extracts a package into `dest` (the game folder itself, e.g. `…/CUSA03173`).
/// Patches and DLC extract over an existing base folder, replacing files.
/// `progress(done, total)` reports bytes of game data written.
pub fn extract(path: &Path, dest: &Path, progress: &mut dyn FnMut(u64, u64)) -> AppResult<PkgInfo> {
    let info = info(path)?;
    let mut f = std::fs::File::open(path)?;
    let (h, _) = read_header(&mut f)?;
    let actual = f.metadata()?.len();
    if h.pkg_size > actual {
        return Err(AppError::msg(format!(
            "el PKG está incompleto: la cabecera declara {} MB y el archivo tiene {} MB",
            h.pkg_size / 1_048_576,
            actual / 1_048_576
        )));
    }
    if h.content_offset + h.content_size > h.pkg_size {
        return Err(AppError::msg("PKG inconsistente (content size)"));
    }
    let entries = read_entries(&mut f, &h)?;
    std::fs::create_dir_all(dest.join("sce_sys"))?;

    // Keys: dk3 from the entry keys, then ekpfs from the image key.
    let dk3_key = rsa_key(
        &keys::PKGDERIVEDKEY3KEYSET_MODULUS,
        &keys::PKGDERIVEDKEY3KEYSET_PRIVATEEXPONENT,
        &keys::PKGDERIVEDKEY3KEYSET_PRIME1,
        &keys::PKGDERIVEDKEY3KEYSET_PRIME2,
    )?;
    let fake_key = rsa_key(
        &keys::FAKEKEYSET_MODULUS,
        &keys::FAKEKEYSET_PRIVATEEXPONENT,
        &keys::FAKEKEYSET_PRIME1,
        &keys::FAKEKEYSET_PRIME2,
    )?;
    let mut dk3: Option<[u8; 32]> = None;
    let mut ekpfs: Option<[u8; 32]> = None;
    if let Some(e) = entries.iter().find(|e| e.id == 0x10) {
        // seed digest (32) + 7 digests (32 each) + 7 keys (256 each)
        let data = read_at(&mut f, e.offset as u64, 32 + 7 * 32 + 7 * 256)?;
        let key3 = &data[32 + 7 * 32 + 3 * 256..32 + 7 * 32 + 4 * 256];
        dk3 = Some(rsa_decrypt32(&dk3_key, key3)?);
    }
    let dk3 = dk3.ok_or_else(|| AppError::msg("el PKG no tiene tabla de claves (entry 0x10)"))?;
    let ivkey_for = |e: &Entry| -> [u8; 32] {
        let mut hs = Sha256::new();
        hs.update(e.raw);
        hs.update(dk3);
        hs.finalize().into()
    };
    if let Some(e) = entries.iter().find(|e| e.id == 0x20) {
        let mut img = read_at(&mut f, e.offset as u64, 256)?;
        aes_cbc_decrypt(&ivkey_for(e), &mut img);
        ekpfs = Some(rsa_decrypt32(&fake_key, &img)?);
    }
    let ekpfs = ekpfs.ok_or_else(|| AppError::msg("el PKG no tiene clave de imagen (entry 0x20)"))?;

    // sce_sys entries (param.sfo, icons, licences…).
    for e in &entries {
        let name = entry_name(e.id).map(|s| s.to_string()).unwrap_or_else(|| format!("{}", e.id));
        if matches!(e.id, 0x1 | 0x10 | 0x20 | 0x80 | 0x100 | 0x200) || e.size == 0 {
            continue; // internal tables, not files of the game
        }
        let mut data = read_at(&mut f, e.offset as u64, e.size as usize)?;
        if (0x400..=0x403).contains(&e.id) {
            aes_cbc_decrypt(&ivkey_for(e), &mut data);
        }
        let target = dest.join("sce_sys").join(&name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(target, data)?;
    }

    // PFS: walk the tree, then copy every file.
    let mut pfs = Pfs::open(f, &h, &ekpfs)?;
    let sb = pfs.meta_block(0)?;
    let superroot = le64(&sb, 0x48);
    let root = pfs.inode(superroot)?;
    if root.mode & 0x4000 == 0 {
        return Err(AppError::msg("el superroot del PFS no es un directorio"));
    }
    let mut files = Vec::new();
    pfs.walk(&root, Path::new(""), &mut files, 0)?;
    let total: u64 = files.iter().map(|(_, n)| n.size).sum();
    let mut done = 0u64;
    progress(0, total);
    for (rel, node) in &files {
        let clean: PathBuf = rel.components().filter(|c| matches!(c, std::path::Component::Normal(_))).collect();
        if clean.as_os_str().is_empty() {
            continue;
        }
        pfs.extract_file(node, &dest.join(clean), &mut |n| {
            done += n;
            progress(done, total);
        })?;
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sfo_parse() {
        // key table at 0x24, data at 0x30, one entry: CATEGORY = "gd"
        let mut b = vec![0u8; 0x40];
        b[0..4].copy_from_slice(b"\0PSF");
        b[8..12].copy_from_slice(&0x24u32.to_le_bytes());
        b[12..16].copy_from_slice(&0x30u32.to_le_bytes());
        b[16..20].copy_from_slice(&1u32.to_le_bytes());
        b[0x14..0x16].copy_from_slice(&0u16.to_le_bytes());
        b[0x16..0x18].copy_from_slice(&0x0204u16.to_le_bytes());
        b[0x18..0x1c].copy_from_slice(&3u32.to_le_bytes());
        b[0x20..0x24].copy_from_slice(&0u32.to_le_bytes());
        b[0x24..0x2d].copy_from_slice(b"CATEGORY\0");
        b[0x30..0x33].copy_from_slice(b"gd\0");
        let m = parse_sfo(&b);
        assert_eq!(m.get("CATEGORY").map(|s| s.as_str()), Some("gd"));
    }

    #[test]
    fn xts_roundtrip_tweak_multiply() {
        // Decrypting zeros with any key must be deterministic and not panic.
        let x = Xts::new(&[7u8; 32], &[1u8; 16]);
        let mut buf = vec![0u8; 0x2000];
        x.decrypt(&mut buf, 5);
        assert_ne!(buf[..16], [0u8; 16]);
    }

    /// Real package: `FREEPORT_TEST_PKG=/path/x.pkg cargo test -- --ignored pkg_real`.
    #[test]
    #[ignore]
    fn pkg_real() {
        let Ok(p) = std::env::var("FREEPORT_TEST_PKG") else { return };
        let dest = std::env::var("FREEPORT_TEST_DEST").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("freeport-pkg-test"));
        let _ = std::fs::remove_dir_all(&dest);
        let i = info(Path::new(&p)).unwrap();
        println!("{i:?}");
        let mut last = 0;
        let r = extract(Path::new(&p), &dest, &mut |d, t| {
            if d / 50_000_000 != last {
                last = d / 50_000_000;
                println!("{d}/{t}");
            }
        })
        .unwrap();
        println!("{r:?}");
        assert!(dest.join("sce_sys/param.sfo").exists());
    }
}
