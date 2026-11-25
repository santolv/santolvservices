CREATE TABLE public.service_user_denials (
    user_id uuid NOT NULL REFERENCES public.users(id) ON DELETE CASCADE,
    service_id text NOT NULL REFERENCES public.service_settings(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, service_id)
);
