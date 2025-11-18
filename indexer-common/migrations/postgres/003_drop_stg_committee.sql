-- This migration drops the legacy staging table added in 002 to maintain checksum compatibility.
-- Safe to run even if the table doesn't exist.

DROP TABLE IF EXISTS stg_committee;
