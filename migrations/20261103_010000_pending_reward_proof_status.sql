-- OP010: expand only the already-existing strict-proof status CHECK.
-- Preserves all legacy rows, status values, indices, grants, and outbox records.
-- Requires the exact previously audited 4-status constraint or an identical
-- already-upgraded 5-status constraint; refuses unrelated schema drift.
DO $op010$
DECLARE
  current_def TEXT;
  old_def CONSTANT TEXT :=
    'CHECK ((status = ANY (ARRAY[''pending''::text, ''confirmed''::text, ''failed''::text, ''expired''::text])))';
  new_def CONSTANT TEXT :=
    'CHECK ((status = ANY (ARRAY[''pending''::text, ''confirmed''::text, ''failed''::text, ''expired''::text, ''awaiting_block_proof''::text])))';
BEGIN
  SELECT pg_get_constraintdef(oid) INTO current_def
  FROM pg_constraint
  WHERE conrelid = 'public.pending_rewards'::regclass
    AND conname = 'ck_pending_rewards_status'
    AND contype = 'c';

  IF current_def = new_def THEN
    RAISE NOTICE 'OP010_ALREADY_MIGRATED_SAME_CONSTRAINT';
  ELSIF current_def = old_def THEN
    ALTER TABLE public.pending_rewards
      DROP CONSTRAINT ck_pending_rewards_status;
    ALTER TABLE public.pending_rewards
      ADD CONSTRAINT ck_pending_rewards_status
      CHECK (status IN (
        'pending','confirmed','failed','expired','awaiting_block_proof'
      ));
    RAISE NOTICE 'OP010_ADDITIVE_STATUS_EXPANSION=PASS';
  ELSE
    RAISE EXCEPTION 'OP010_HOLD_UNEXPECTED_PENDING_REWARD_STATUS_CONSTRAINT';
  END IF;
END
$op010$;
