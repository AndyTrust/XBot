//! Testo sensibile: niente `Debug` in chiaro, niente `Serialize`, azzerato quando cade.

use serde::{Deserialize, Deserializer};
use zeroize::Zeroize;

/// Un valore sensibile (password, codice, campi della cassaforte).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct SegretoTesto(String);

impl SegretoTesto {
    pub fn nuovo(valore: impl Into<String>) -> Self {
        Self(valore.into())
    }

    /// Il valore in chiaro: da usare solo dove serve davvero.
    pub fn esponi(&self) -> &str {
        &self.0
    }

    pub fn lunghezza(&self) -> usize {
        self.0.len()
    }

    pub fn vuoto(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for SegretoTesto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SegretoTesto(*** {} byte)", self.0.len())
    }
}

impl Drop for SegretoTesto {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl From<String> for SegretoTesto {
    fn from(valore: String) -> Self {
        Self(valore)
    }
}

impl<'de> Deserialize<'de> for SegretoTesto {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d).map(Self)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn debug_non_mostra_il_valore() {
        let s = SegretoTesto::nuovo("parola-segreta");
        let d = format!("{s:?}");
        assert!(!d.contains("parola"));
        assert_eq!(s.esponi(), "parola-segreta");
    }
}
