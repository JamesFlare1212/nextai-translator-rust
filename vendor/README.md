`gpui-component/` 是 crates.io 发布的 `gpui-component 0.11.0`（Apache-2.0，原始 `.crate` SHA-256：`cdfec0424e4d9d78dcec4b4410e310b43ac1c4ef3efb971cfb63f5855754798b`）。该版本未公开 Select / Combobox 的菜单面板样式；根 `Cargo.toml` 用 `[patch.crates-io]` 固定此副本，不修改开发机的 Cargo 缓存。

相对发布包，唯一修改在 `src/searchable_list/state.rs::menu_popup`：保留列表项主题半径，把非直角主题的面板半径增加列表内距 `DROPDOWN_LIST_PADDING`。升级时先检查上游是否已支持同心圆角；能移除此补丁时，运行应用的 `language_menu_surface_follows_the_inset_of_the_selected_row` 绘制回归测试。
