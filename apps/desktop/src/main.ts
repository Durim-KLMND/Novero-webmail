import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
import { applyTheme, cachedTheme } from "./lib/theme";
import { watchTitlebar } from "./lib/frame.svelte";
import { guardModalScroll } from "./lib/modal";
import { watchPlan } from "./lib/pro.svelte";
import { watchLinks } from "./lib/hoverlink.svelte";

applyTheme(cachedTheme());
watchTitlebar();
watchPlan();
guardModalScroll();
watchLinks(document);

const target = document.getElementById("app");
if (!target) throw new Error("missing #app element");

export default mount(App, { target });
