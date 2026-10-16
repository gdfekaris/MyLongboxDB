-- The Canadian price printed on a cover beside the US one, in Canadian
-- cents. It is a fact about the edition, never money spent, so it has no
-- currency of its own in the exchange-rate table and is never converted.
-- Existing editions start with none.
ALTER TABLE edition ADD COLUMN canadian_cover_price_cents INTEGER
    CHECK (canadian_cover_price_cents >= 0);
