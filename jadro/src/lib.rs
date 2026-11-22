//! Kometa – vlastné jadro na sťahovanie.
//!
//! Vrstvy: `klient` (HTTP), `http` (súbory po úsekoch s pokračovaním),
//! `kusky` (zoznamy kúskov streamov), `hls`, `dash`, `spajanie` (stopy do jedného súboru).
//! Šifrovaný obsah (DRM, kľúče v streame) jadro zámerne nesťahuje.

pub mod chyba;
pub mod dash;
pub mod hls;
pub mod http;
pub mod klient;
pub mod kusky;
pub mod spajanie;
pub mod stream;
pub mod udalosti;
