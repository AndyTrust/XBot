//! Le enumerazioni dello schema come tipi Rust (testo uguale ai CHECK di `schema.sql`).

use crate::{Errore, Risposta};

macro_rules! enum_testo {
    ($nome:ident { $($variante:ident => $testo:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $nome { $($variante),+ }

        impl $nome {
            pub const TUTTI: &'static [$nome] = &[$($nome::$variante),+];

            pub fn come_testo(self) -> &'static str {
                match self { $($nome::$variante => $testo),+ }
            }

            pub fn da_testo(testo: &str) -> Risposta<Self> {
                match testo {
                    $($testo => Ok($nome::$variante),)+
                    altro => Err(Errore::non_valida(format!(
                        concat!(stringify!($nome), " sconosciuto: {}"), altro
                    ))),
                }
            }
        }

        impl std::fmt::Display for $nome {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.come_testo())
            }
        }
    };
}

enum_testo!(GenereRiga {
    Messaggio => "messaggio",
    ChiamataStrumento => "chiamata_strumento",
    Richiesta => "richiesta",
    AllegatoUtente => "allegato_utente",
    Avviso => "avviso",
    Evento => "evento",
    Ragionamento => "ragionamento",
    Diff => "diff",
    Media => "media",
    TempoTurno => "tempo_turno",
    Esito => "esito",
    ChiamataVoce => "chiamata_voce",
    AppMcp => "app_mcp",
});

enum_testo!(Ruolo {
    Utente => "utente",
    Assistente => "assistente",
    Sistema => "sistema",
    Collega => "collega",
});

enum_testo!(StatoInvio {
    InCoda => "in_coda",
    Accettato => "accettato",
    Duplicato => "duplicato",
    Rifiutato => "rifiutato",
    Consegnato => "consegnato",
    Fallito => "fallito",
});

enum_testo!(Consegna {
    InCodaProssimoTurno => "in_coda_prossimo_turno",
    InseritoNelTurno => "inserito_nel_turno",
    Trattenuto => "trattenuto",
    AvviatoContinuazione => "avviato_continuazione",
    SalvatoPerDopo => "salvato_per_dopo",
    ContinuazioneBloccata => "continuazione_bloccata",
});

enum_testo!(StatoTurno {
    InCorso => "in_corso",
    Riuscito => "riuscito",
    Fallito => "fallito",
    Annullato => "annullato",
    Interrotto => "interrotto",
});

enum_testo!(StatoLavoro {
    InCoda => "in_coda",
    Preso => "preso",
    InCorso => "in_corso",
    Riuscito => "riuscito",
    Fallito => "fallito",
    Annullato => "annullato",
    Scaduto => "scaduto",
});

enum_testo!(PermessoEsecuzione {
    Always => "always",
    Ask => "ask",
    Never => "never",
});

enum_testo!(TipoRichiesta {
    PermessoLocale => "permesso_locale",
    RevisioneComando => "revisione_comando",
    Modulo => "modulo",
    Segreto => "segreto",
    Cassaforte => "cassaforte",
    Domanda => "domanda",
    Collegamento => "collegamento",
    SpostamentoSessione => "spostamento_sessione",
});

enum_testo!(StatoRichiesta {
    Aperta => "aperta",
    Approvata => "approvata",
    Negata => "negata",
    Scaduta => "scaduta",
    Ritirata => "ritirata",
});

enum_testo!(GenereCassaforte {
    Accesso => "accesso",
    Carta => "carta",
    Contatto => "contatto",
    Segreto => "segreto",
});

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn andata_e_ritorno() {
        for g in GenereRiga::TUTTI {
            assert_eq!(GenereRiga::da_testo(g.come_testo()).unwrap(), *g);
        }
        assert_eq!(
            serde_json::to_string(&Consegna::InCodaProssimoTurno).unwrap(),
            "\"in_coda_prossimo_turno\""
        );
        assert!(StatoTurno::da_testo("boh").is_err());
    }
}
