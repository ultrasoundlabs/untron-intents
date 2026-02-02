alter table solver.hub_userops
    add column if not exists receipt jsonb;

