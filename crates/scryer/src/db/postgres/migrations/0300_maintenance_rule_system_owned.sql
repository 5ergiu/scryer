-- A system-owned maintenance rule ships with Scryer: its name, matcher and
-- action are fixed, and it cannot be deleted.
ALTER TABLE maintenance_rule_sets
    ADD COLUMN system_owned boolean NOT NULL DEFAULT false;
