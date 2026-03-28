/// Bounty Board extension shared configuration.
///
/// Publishes a shared `ExtensionConfig` for admin rules and an `AdminCap` for
/// configuration. Other modules in this package use `XAuth` as the typed witness
/// for StorageUnit extension authorization.
module bounty_board::config {
    use sui::dynamic_field as df;

    public struct ExtensionConfig has key {
        id: UID,
    }

    public struct AdminCap has key, store {
        id: UID,
    }

    /// Witness type for StorageUnit extension authorization.
    public struct XAuth has drop {}

    fun init(ctx: &mut TxContext) {
        let admin_cap = AdminCap { id: object::new(ctx) };
        transfer::transfer(admin_cap, ctx.sender());

        let config = ExtensionConfig { id: object::new(ctx) };
        transfer::share_object(config);
    }

    // === Dynamic field helpers ===

    public fun has_rule<K: copy + drop + store>(config: &ExtensionConfig, key: K): bool {
        df::exists_(&config.id, key)
    }

    public fun borrow_rule<K: copy + drop + store, V: store>(config: &ExtensionConfig, key: K): &V {
        df::borrow(&config.id, key)
    }

    public fun borrow_rule_mut<K: copy + drop + store, V: store>(
        config: &mut ExtensionConfig,
        _: &AdminCap,
        key: K,
    ): &mut V {
        df::borrow_mut(&mut config.id, key)
    }

    public fun add_rule<K: copy + drop + store, V: store>(
        config: &mut ExtensionConfig,
        _: &AdminCap,
        key: K,
        value: V,
    ) {
        df::add(&mut config.id, key, value);
    }

    public fun set_rule<K: copy + drop + store, V: store + drop>(
        config: &mut ExtensionConfig,
        _: &AdminCap,
        key: K,
        value: V,
    ) {
        if (df::exists_(&config.id, copy key)) {
            let _old: V = df::remove(&mut config.id, copy key);
        };
        df::add(&mut config.id, key, value);
    }

    public fun remove_rule<K: copy + drop + store, V: store>(
        config: &mut ExtensionConfig,
        _: &AdminCap,
        key: K,
    ): V {
        df::remove(&mut config.id, key)
    }

    /// Mint an `XAuth` witness. Restricted to this package.
    public(package) fun x_auth(): XAuth {
        XAuth {}
    }

    #[test_only]
    public fun init_for_testing(ctx: &mut TxContext) {
        init(ctx);
    }
}
