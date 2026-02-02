alter table solver.hub_userops
    add column if not exists actual_gas_cost_wei numeric,
    add column if not exists actual_gas_used numeric;

create index if not exists hub_userops_kind_updated_at_idx
    on solver.hub_userops(kind, updated_at desc);
