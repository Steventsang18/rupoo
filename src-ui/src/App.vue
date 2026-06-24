<script setup lang="ts">
import Layout from "./components/Layout.vue";
import { useAppStore } from "./stores/app";

const app = useAppStore();
app.init();
</script>

<template>
  <div class="app-root">
    <Layout />
  </div>
</template>

<style>
/* Design system is loaded via global.css in index.html */
.app-root {
  width: 100vw;
  height: 100vh;
  overflow: hidden;
}
</style>
