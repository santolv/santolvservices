-- Office editors (WordCraft, GridCraft, DeckCraft, PdfCraft). Their documents are Drive files, so
-- the services keep no tables of their own yet and require Drive. Installed disabled, like every
-- new service; an administrator enables them with `services enable <id>`.
INSERT INTO public.service_settings (id, enabled) VALUES
    ('documents', false),
    ('spreadsheets', false),
    ('presentations', false),
    ('pdf', false);
