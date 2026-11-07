-- Add indexes for resource dependency point lookups.

CREATE INDEX api_collect_resource_dependency_trade_no
    ON api_collect (resource_dependency_trade_no)
    WHERE resource_dependency_trade_no IS NOT NULL;

CREATE INDEX api_withdraws_resource_dependency_trade_no
    ON api_withdraws (trade_type, resource_dependency_trade_no)
    WHERE resource_dependency_trade_no IS NOT NULL;
