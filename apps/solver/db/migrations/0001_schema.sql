create schema if not exists solver;

create table if not exists solver.schema_migrations (
    version int primary key,
    applied_at timestamptz not null default now()
);

