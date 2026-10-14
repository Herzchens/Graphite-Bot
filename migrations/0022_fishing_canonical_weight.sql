BEGIN;

ALTER TABLE item_instances
    ADD COLUMN catch_weight_scaled_grams BIGINT,
    ADD CONSTRAINT item_instances_catch_weight_scaled_positive
        CHECK (catch_weight_scaled_grams IS NULL OR catch_weight_scaled_grams > 0),
    ADD CONSTRAINT item_instances_catch_weight_projection_consistent
        CHECK (
            catch_weight_scaled_grams IS NULL
            OR (
                catch_weight_grams IS NOT NULL
                AND catch_weight_grams =
                    catch_weight_scaled_grams / 1000000000000
                    + CASE
                        WHEN catch_weight_scaled_grams % 1000000000000 >= 500000000000
                            THEN 1
                        ELSE 0
                      END
            )
        );

COMMENT ON COLUMN item_instances.catch_weight_scaled_grams IS
    'Canonical FishInstance weight in 1e-12 grams; catch_weight_grams remains the round-half-up CatchBag capacity/display projection';

COMMIT;
