alter table solver.tron_tx_costs
    add column if not exists intent_type smallint;

-- Backfill from the owning job's intent type when available.
update solver.tron_tx_costs c
set intent_type = j.intent_type
from solver.jobs j
where c.intent_type is null
  and c.job_id is not null
  and j.job_id = c.job_id;

create index if not exists tron_tx_costs_intent_type_updated_at_idx
    on solver.tron_tx_costs(intent_type, updated_at desc);

