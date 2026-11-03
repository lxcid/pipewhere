-- A published publication names the post it made, per 00002 D2 and D3. Nothing
-- moves a publication out of `published`, so neither column changes once set.
ALTER TABLE pipewhere.publication
    ADD COLUMN remote_id text,
    ADD COLUMN remote_url text,
    ADD CONSTRAINT publication_remote_id_check CHECK ((state = 'published') = (remote_id IS NOT NULL));
