// Verbatim 1.1.2 try_decrypt_field body from d3fe0d1cc07b789a1bc6a43d0a997b8381faef50,
// src-tauri/src/store/settings.rs:1061. Only visibility/type import is adapted for tests.
use crate::store::secrets::SecretStore;

pub(super) struct Legacy112;

impl Legacy112 {
    pub(super) fn try_decrypt_field(store: &SecretStore, field: &mut String) {
        if field.is_empty() {
            return;
        }
        if let Ok(decrypted) = store.decrypt_hex(field) {
            *field = decrypted;
        }
        // If decrypt fails, the field is probably still plaintext (legacy) — keep as-is
    }
}
