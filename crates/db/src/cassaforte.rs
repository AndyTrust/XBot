//! La cassaforte: chiave da passphrase (Argon2id), campi cifrati con XChaCha20-Poly1305.
//!
//! Formato di un blob cifrato: `[versione=1][nonce 24 byte][testo cifrato + tag 16 byte]`.
//! Dati associati autenticati: `plancia.cassaforte.v1|<id della voce>`, così un blob
//! spostato su un'altra voce non si decifra. Le etichette `plancia.*` vengono dal primo nome del
//! progetto: entrano nella cifratura, quindi non si cambiano (i dati già cifrati non si
//! leggerebbero più).

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, Generate, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use serde_json::Value;
use xbot_core::{
    Codice, Errore, GenereCassaforte, Id, Ms, Risposta, SegretoTesto, nuovo_id, ora_ms,
};
use zeroize::Zeroize;

use crate::util::RisultatoDb;

const VERSIONE: u8 = 1;
const NONCE: usize = 24;
const ID_VERIFICA: &str = "cassaforte_chiave:verifica";
const TESTO_VERIFICA: &[u8] = b"plancia-cassaforte-verifica"; // etichetta storica della cifratura: non si cambia

/// Parametri Argon2id (raccomandazione OWASP: 19 MiB, 2 passate, 1 corsia).
#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ParametriKdf {
    pub m: u32,
    pub t: u32,
    pub p: u32,
}

pub const PARAMETRI: ParametriKdf = ParametriKdf {
    m: 19_456,
    t: 2,
    p: 1,
};

/// La chiave principale aperta. Azzerata quando cade, mai stampata.
pub struct Chiave([u8; 32]);

impl Chiave {
    /// Una chiave da byte già pronti (es. derivata altrove dal server).
    pub fn da_byte(b: [u8; 32]) -> Self {
        Self(b)
    }
}

impl std::fmt::Debug for Chiave {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Chiave(***)")
    }
}

impl Drop for Chiave {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Deriva una chiave da una passphrase con Argon2id.
pub fn deriva(passphrase: &SegretoTesto, sale: &[u8], par: ParametriKdf) -> Risposta<Chiave> {
    let params = Params::new(par.m, par.t, par.p, Some(32))
        .map_err(|e| Errore::nuovo(Codice::Cifratura, format!("parametri Argon2: {e}")))?;
    let mut uscita = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.esponi().as_bytes(), sale, &mut uscita)
        .map_err(|e| Errore::nuovo(Codice::Cifratura, format!("Argon2: {e}")))?;
    Ok(Chiave(uscita))
}

fn dati_associati(id: &str) -> Vec<u8> {
    let mut v = b"plancia.cassaforte.v1|".to_vec();
    v.extend_from_slice(id.as_bytes());
    v
}

fn cifratore(chiave: &Chiave) -> Risposta<XChaCha20Poly1305> {
    XChaCha20Poly1305::new_from_slice(&chiave.0)
        .map_err(|_| Errore::nuovo(Codice::Cifratura, "chiave di lunghezza sbagliata"))
}

/// Cifra `chiaro` per la voce `id`, con nonce casuale.
pub fn cifra(chiave: &Chiave, id: &str, chiaro: &[u8]) -> Risposta<Vec<u8>> {
    let nonce = XNonce::generate();
    let aad = dati_associati(id);
    let cifrato = cifratore(chiave)?
        .encrypt(
            &nonce,
            Payload {
                msg: chiaro,
                aad: &aad,
            },
        )
        .map_err(|_| Errore::nuovo(Codice::Cifratura, "cifratura fallita"))?;
    let mut blob = Vec::with_capacity(1 + NONCE + cifrato.len());
    blob.push(VERSIONE);
    blob.extend_from_slice(nonce.as_slice());
    blob.extend_from_slice(&cifrato);
    Ok(blob)
}

/// Decifra un blob della voce `id`. Qualunque manomissione dà errore `cifratura`.
pub fn decifra(chiave: &Chiave, id: &str, blob: &[u8]) -> Risposta<Vec<u8>> {
    if blob.len() < 1 + NONCE + 16 || blob[0] != VERSIONE {
        return Err(Errore::nuovo(Codice::Cifratura, "blob cifrato non valido"));
    }
    let nonce = XNonce::try_from(&blob[1..=NONCE])
        .map_err(|_| Errore::nuovo(Codice::Cifratura, "nonce non valido"))?;
    let aad = dati_associati(id);
    cifratore(chiave)?
        .decrypt(
            &nonce,
            Payload {
                msg: &blob[1 + NONCE..],
                aad: &aad,
            },
        )
        .map_err(|_| Errore::nuovo(Codice::Cifratura, "decifratura rifiutata"))
}

/// Imposta la passphrase la prima volta: sale casuale, parametri, blob di verifica.
pub fn imposta_passphrase(conn: &Connection, passphrase: &SegretoTesto) -> Risposta<Chiave> {
    if passphrase.lunghezza() < 8 {
        return Err(Errore::non_valida("la passphrase vuole almeno 8 caratteri"));
    }
    let esiste: bool = conn
        .query_row("SELECT EXISTS(SELECT 1 FROM cassaforte_chiave)", [], |r| {
            r.get(0)
        })
        .db()?;
    if esiste {
        return Err(Errore::non_valida("la passphrase è già impostata"));
    }
    let sale: [u8; 16] = rand::random();
    let chiave = deriva(passphrase, &sale, PARAMETRI)?;
    let verifica = cifra(&chiave, ID_VERIFICA, TESTO_VERIFICA)?;
    let parametri = serde_json::to_string(&PARAMETRI)
        .map_err(|e| Errore::interno(format!("parametri: {e}")))?;
    conn.execute(
        "INSERT INTO cassaforte_chiave (id, kdf, sale, parametri_json, verifica, creata_ms)
         VALUES (1, 'argon2id', ?1, ?2, ?3, ?4)",
        params![sale.as_slice(), parametri, verifica, ora_ms()],
    )
    .db()?;
    Ok(chiave)
}

/// Apre la cassaforte: passphrase sbagliata → `passphrase_sbagliata`.
pub fn apri(conn: &Connection, passphrase: &SegretoTesto) -> Risposta<Chiave> {
    let riga: Option<(Vec<u8>, String, Vec<u8>)> = conn
        .query_row(
            "SELECT sale, parametri_json, verifica FROM cassaforte_chiave WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .db()?;
    let (sale, parametri, verifica) = riga
        .ok_or_else(|| Errore::nuovo(Codice::CassaforteNonImpostata, "cassaforte non impostata"))?;
    let par: ParametriKdf = serde_json::from_str(&parametri)
        .map_err(|e| Errore::interno(format!("parametri della cassaforte illeggibili: {e}")))?;
    let chiave = deriva(passphrase, &sale, par)?;
    match decifra(&chiave, ID_VERIFICA, &verifica) {
        Ok(t) if t == TESTO_VERIFICA => Ok(chiave),
        _ => Err(Errore::nuovo(
            Codice::PassphraseSbagliata,
            "passphrase sbagliata",
        )),
    }
}

/// Una voce nuova. `campi` è il JSON sensibile (password, numero, valore…).
#[derive(Debug, Clone)]
pub struct NuovaVoce {
    pub genere: GenereCassaforte,
    pub titolo: String,
    pub siti: Vec<String>,
    pub riferimento: Value,
    pub campi: SegretoTesto,
    pub origine: Option<String>,
}

/// Quello che si vede di una voce senza chiave: mai i campi cifrati.
#[derive(Debug, Clone, Serialize)]
pub struct VoceRiferimento {
    pub id: Id,
    pub genere: String,
    pub titolo: String,
    pub siti: Value,
    pub riferimento: Value,
    pub origine: Option<String>,
    pub creata_ms: Ms,
    pub aggiornata_ms: Ms,
    pub ultimo_uso_ms: Option<Ms>,
}

const COLONNE: &str = "id, genere, titolo, siti_json, riferimento_json, origine, creata_ms, \
    aggiornata_ms, ultimo_uso_ms";

fn da_riga(r: &Row<'_>) -> rusqlite::Result<VoceRiferimento> {
    let siti: String = r.get(3)?;
    let rif: String = r.get(4)?;
    Ok(VoceRiferimento {
        id: r.get(0)?,
        genere: r.get(1)?,
        titolo: r.get(2)?,
        siti: serde_json::from_str(&siti).unwrap_or(Value::Array(vec![])),
        riferimento: serde_json::from_str(&rif).unwrap_or(Value::Null),
        origine: r.get(5)?,
        creata_ms: r.get(6)?,
        aggiornata_ms: r.get(7)?,
        ultimo_uso_ms: r.get(8)?,
    })
}

pub fn inserisci_voce(conn: &Connection, chiave: &Chiave, v: &NuovaVoce) -> Risposta<Id> {
    if v.titolo.trim().is_empty() {
        return Err(Errore::non_valida("il titolo è obbligatorio"));
    }
    let id = nuovo_id();
    let cifrati = cifra(chiave, &id, v.campi.esponi().as_bytes())?;
    let siti = serde_json::to_string(&v.siti).map_err(|e| Errore::interno(e.to_string()))?;
    let ora = ora_ms();
    conn.execute(
        "INSERT INTO cassaforte_voce (id, genere, titolo, siti_json, riferimento_json,
           campi_cifrati, origine, creata_ms, aggiornata_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            id,
            v.genere.come_testo(),
            v.titolo.trim(),
            siti,
            v.riferimento.to_string(),
            cifrati,
            v.origine,
            ora
        ],
    )
    .db()?;
    Ok(id)
}

/// Le voci (solo riferimento), di un genere o tutte.
pub fn elenca(
    conn: &Connection,
    genere: Option<GenereCassaforte>,
) -> Risposta<Vec<VoceRiferimento>> {
    let mut st = conn
        .prepare(&format!(
            "SELECT {COLONNE} FROM cassaforte_voce WHERE (?1 IS NULL OR genere = ?1)
             ORDER BY genere, titolo"
        ))
        .db()?;
    let v = st
        .query_map([genere.map(GenereCassaforte::come_testo)], da_riga)
        .db()?
        .collect::<rusqlite::Result<Vec<_>>>()
        .db()?;
    Ok(v)
}

pub fn leggi_riferimento(conn: &Connection, id: &str) -> Risposta<VoceRiferimento> {
    conn.query_row(
        &format!("SELECT {COLONNE} FROM cassaforte_voce WHERE id = ?1"),
        [id],
        da_riga,
    )
    .optional()
    .db()?
    .ok_or_else(|| Errore::nuovo(Codice::VoceNonTrovata, "voce non trovata"))
}

/// I campi sensibili in chiaro (JSON). Aggiorna l'ultimo uso.
pub fn leggi_campi(conn: &Connection, id: &str, chiave: &Chiave) -> Risposta<SegretoTesto> {
    let blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT campi_cifrati FROM cassaforte_voce WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .db()?;
    let blob = blob.ok_or_else(|| Errore::nuovo(Codice::VoceNonTrovata, "voce non trovata"))?;
    let mut chiaro = decifra(chiave, id, &blob)?;
    let testo = String::from_utf8(chiaro.clone())
        .map_err(|_| Errore::nuovo(Codice::Cifratura, "campi non in UTF-8"));
    chiaro.zeroize();
    conn.execute(
        "UPDATE cassaforte_voce SET ultimo_uso_ms = ?2 WHERE id = ?1",
        params![id, ora_ms()],
    )
    .db()?;
    testo.map(SegretoTesto::from)
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    fn chiave_veloce() -> Chiave {
        Chiave::da_byte([7u8; 32])
    }

    #[test]
    fn cifra_e_decifra() {
        let k = chiave_veloce();
        let b = cifra(&k, "v1", b"segreto").unwrap();
        assert_eq!(decifra(&k, "v1", &b).unwrap(), b"segreto");
        let b2 = cifra(&k, "v1", b"segreto").unwrap();
        assert_ne!(b, b2, "nonce casuale: due cifrature diverse");
    }

    #[test]
    fn manomissione_di_un_byte_rifiutata() {
        let k = chiave_veloce();
        let b = cifra(&k, "v1", b"segreto").unwrap();
        for i in 0..b.len() {
            let mut m = b.clone();
            m[i] ^= 0x01;
            assert!(
                decifra(&k, "v1", &m).is_err(),
                "byte {i} manomesso accettato"
            );
        }
        assert!(
            decifra(&k, "v2", &b).is_err(),
            "blob spostato su un'altra voce"
        );
        assert!(decifra(&Chiave::da_byte([8u8; 32]), "v1", &b).is_err());
    }

    #[test]
    fn passphrase_giusta_e_sbagliata() {
        let db = crate::db_prova();
        db.con(|c| {
            let giusta = SegretoTesto::nuovo("cavallo batteria graffetta");
            assert_eq!(
                apri(c, &giusta).unwrap_err().codice,
                Codice::CassaforteNonImpostata
            );
            let k = imposta_passphrase(c, &giusta)?;
            assert!(imposta_passphrase(c, &giusta).is_err());
            let id = inserisci_voce(
                c,
                &k,
                &NuovaVoce {
                    genere: GenereCassaforte::Accesso,
                    titolo: "Posta".into(),
                    siti: vec!["https://posta.example".into()],
                    riferimento: json!({"nome_utente": "mario"}),
                    campi: SegretoTesto::nuovo(r#"{"password":"p4ss"}"#),
                    origine: Some("manuale".into()),
                },
            )?;
            let k2 = apri(c, &giusta)?;
            assert_eq!(leggi_campi(c, &id, &k2)?.esponi(), r#"{"password":"p4ss"}"#);
            let e = apri(c, &SegretoTesto::nuovo("cavallo sbagliato")).unwrap_err();
            assert_eq!(e.codice, Codice::PassphraseSbagliata);
            let r = leggi_riferimento(c, &id)?;
            assert_eq!(r.riferimento["nome_utente"], "mario");
            assert!(r.ultimo_uso_ms.is_some());
            assert_eq!(elenca(c, Some(GenereCassaforte::Accesso))?.len(), 1);
            assert!(elenca(c, Some(GenereCassaforte::Carta))?.is_empty());
            let s = serde_json::to_string(&r).unwrap();
            assert!(!s.contains("p4ss"));
            Ok(())
        })
        .unwrap();
    }
}
