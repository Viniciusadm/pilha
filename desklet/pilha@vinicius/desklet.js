const Desklet = imports.ui.desklet;
const Settings = imports.ui.settings;
const Util = imports.misc.util;
const St = imports.gi.St;
const Gio = imports.gi.Gio;
const GLib = imports.gi.GLib;
const Pango = imports.gi.Pango;

const PRIORITY_LABELS = {
    urgent: "URGENTE",
    high: "alta",
    normal: "normal",
    low: "baixa",
};

const DEBOUNCE_MS = 400;

class PilhaDesklet extends Desklet.Desklet {
    constructor(metadata, deskletId) {
        super(metadata, deskletId);

        this.settings = new Settings.DeskletSettings(this, metadata.uuid, deskletId);
        this.settings.bind("command", "command", () => this._refresh());
        this.settings.bind("refresh-interval", "refreshInterval", () => this._scheduleRefresh());
        this.settings.bind("max-items", "maxItems", () => this._refresh());
        this.settings.bind("width", "width", () => this._applyWidth());

        this._refreshTimerId = 0;
        this._debounceTimerId = 0;
        this._running = false;
        this._pending = false;

        this._buildLayout();
        this._watchDatabase();
        this._scheduleRefresh();
        this._refresh();
    }

    _buildLayout() {
        this._root = new St.BoxLayout({ vertical: true, style_class: "pilha-root" });

        const header = new St.BoxLayout({ style_class: "pilha-header" });
        header.add_child(new St.Label({ text: "Pilha", style_class: "pilha-title" }));
        this._countLabel = new St.Label({ text: "", style_class: "pilha-count", y_align: St.Align.MIDDLE });
        header.add_child(this._countLabel);
        header.add_child(new St.Widget({ x_expand: true }));

        const openButton = new St.Button({ label: "+", style_class: "pilha-header-button" });
        openButton.connect("clicked", () => this._openTerminal());
        header.add_child(openButton);

        const refreshButton = new St.Button({ label: "⟳", style_class: "pilha-header-button" });
        refreshButton.connect("clicked", () => this._refresh());
        header.add_child(refreshButton);

        this._root.add_child(header);

        this._list = new St.BoxLayout({ vertical: true });
        this._root.add_child(this._list);

        this._applyWidth();
        this.setContent(this._root);
    }

    _applyWidth() {
        if (this._root) {
            this._root.set_width(this.width);
        }
    }

    _binary() {
        const configured = (this.command || "").trim();
        if (configured) {
            return configured.replace(/^~/, GLib.get_home_dir());
        }
        const found = GLib.find_program_in_path("t");
        if (found) {
            return found;
        }
        return GLib.build_filenamev([GLib.get_home_dir(), ".cargo", "bin", "t"]);
    }

    _databaseDir() {
        return GLib.build_filenamev([GLib.get_user_data_dir(), "pilha"]);
    }

    _watchDatabase() {
        const dirPath = this._databaseDir();
        GLib.mkdir_with_parents(dirPath, 0o755);
        try {
            this._monitor = Gio.File.new_for_path(dirPath).monitor_directory(Gio.FileMonitorFlags.NONE, null);
            this._monitor.connect("changed", () => this._debouncedRefresh());
        } catch (error) {
            global.logError("pilha desklet: " + error);
        }
    }

    _debouncedRefresh() {
        if (this._debounceTimerId) {
            return;
        }
        this._debounceTimerId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, DEBOUNCE_MS, () => {
            this._debounceTimerId = 0;
            this._refresh();
            return GLib.SOURCE_REMOVE;
        });
    }

    _scheduleRefresh() {
        if (this._refreshTimerId) {
            GLib.source_remove(this._refreshTimerId);
        }
        const seconds = Math.max(5, this.refreshInterval || 20);
        this._refreshTimerId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, seconds, () => {
            this._refresh();
            return GLib.SOURCE_CONTINUE;
        });
    }

    _refresh() {
        if (this._running) {
            this._pending = true;
            return;
        }
        this._running = true;
        this._run(["ls", "--json"], (stdout, stderr, exitCode) => {
            this._running = false;
            if (exitCode !== 0) {
                this._renderError((stderr || "falha ao executar t").trim());
            } else {
                try {
                    this._render(JSON.parse(stdout));
                } catch (error) {
                    this._renderError("JSON inválido: " + error.message);
                }
            }
            if (this._pending) {
                this._pending = false;
                this._refresh();
            }
        });
    }

    _run(args, callback) {
        try {
            Util.spawnCommandLineAsyncIO("", callback, { argv: [this._binary()].concat(args) });
        } catch (error) {
            this._running = false;
            this._renderError("não encontrei o binário t: " + error.message);
        }
    }

    _clearList() {
        this._list.destroy_all_children();
    }

    _renderError(message) {
        this._clearList();
        this._countLabel.set_text("");
        this._list.add_child(this._wrappedLabel(message, "pilha-error"));
    }

    _render(todos) {
        this._clearList();
        this._countLabel.set_text(todos.length ? String(todos.length) : "");

        if (!todos.length) {
            this._list.add_child(new St.Label({ text: "Nada na pilha 🎉", style_class: "pilha-empty" }));
            return;
        }

        const limit = Math.max(1, this.maxItems || 12);
        todos.slice(0, limit).forEach((todo) => this._list.add_child(this._buildRow(todo)));

        if (todos.length > limit) {
            this._list.add_child(new St.Label({ text: "+ " + (todos.length - limit) + " na pilha", style_class: "pilha-more" }));
        }
    }

    _buildRow(todo) {
        const row = new St.BoxLayout({ style_class: "pilha-row", reactive: true, track_hover: true });

        const priority = new St.Label({
            text: PRIORITY_LABELS[todo.priority] || todo.priority,
            style_class: "pilha-priority pilha-priority-" + todo.priority,
            y_align: St.Align.START,
        });
        row.add_child(priority);

        const body = new St.BoxLayout({ vertical: true, x_expand: true, style_class: "pilha-item-body" });
        const title = this._wrappedLabel(todo.title, "pilha-item-title");
        body.add_child(title);

        if (todo.links && todo.links.length) {
            const links = new St.BoxLayout({ style_class: "pilha-links" });
            todo.links.forEach((link) => {
                const button = new St.Button({
                    label: link.label || link.url,
                    style_class: "pilha-link pilha-link-" + link.kind,
                });
                button.connect("clicked", () => this._openUri(link.url));
                links.add_child(button);
            });
            body.add_child(links);
        }
        row.add_child(body);

        const doneButton = new St.Button({ label: "✓", style_class: "pilha-done-button" });
        doneButton.connect("clicked", () => this._run(["done", String(todo.id)], () => this._refresh()));
        row.add_child(doneButton);

        return row;
    }

    _wrappedLabel(text, styleClass) {
        const label = new St.Label({ text: text, style_class: styleClass, x_expand: true });
        label.clutter_text.line_wrap = true;
        label.clutter_text.line_wrap_mode = Pango.WrapMode.WORD_CHAR;
        label.clutter_text.ellipsize = Pango.EllipsizeMode.NONE;
        return label;
    }

    _openUri(uri) {
        try {
            Gio.AppInfo.launch_default_for_uri(uri, null);
        } catch (error) {
            global.logError("pilha desklet: " + error);
        }
    }

    _openTerminal() {
        Util.spawn(["x-terminal-emulator", "-e", this._binary()]);
    }

    on_desklet_removed() {
        if (this._refreshTimerId) {
            GLib.source_remove(this._refreshTimerId);
            this._refreshTimerId = 0;
        }
        if (this._debounceTimerId) {
            GLib.source_remove(this._debounceTimerId);
            this._debounceTimerId = 0;
        }
        if (this._monitor) {
            this._monitor.cancel();
            this._monitor = null;
        }
        this.settings.finalize();
    }
}

function main(metadata, deskletId) {
    return new PilhaDesklet(metadata, deskletId);
}
