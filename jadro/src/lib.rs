//! Kometa – vlastné jadro na sťahovanie.
//!
//! Vrstvy: `klient` (HTTP), `http` (súbory po úsekoch s pokračovaním),
//! `kusky` (zoznamy kúskov streamov), `hls`, `dash`, `media` (vlastné čítanie a zápis MKV/MP4),
//! `spajanie` (stopy do jedného súboru) a `web` (skener stránok, Vimeo a ďalšie prehrávače).
//! Šifrovaný obsah (DRM, kľúče v streame) jadro zámerne nesťahuje.

pub mod chyba;
pub mod dash;
pub mod hls;
pub mod http;
pub mod klient;
pub mod kusky;
pub mod media;
pub mod spajanie;
pub mod stream;
pub mod udalosti;
pub mod web;
