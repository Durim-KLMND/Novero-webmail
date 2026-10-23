import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
import { applyTheme, cachedTheme } from "./lib/ui/theme";
import { watchTitlebar } from "./lib/shell/frame.svelte";
import { guardModalScroll } from "./lib/ui/modal";
import { floatScrollbars } from "./lib/ui/scrollbars";
import { watchPlan } from "./lib/plan/pro.svelte";

applyTheme(cachedTheme());
watchTitlebar();
watchPlan();
guardModalScroll();
floatScrollbars();

const target = document.getElementById("app");
if (!target) throw new Error("missing #app element");

export default mount(App, { target });
