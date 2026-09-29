<script setup>
import { onBeforeUnmount, onMounted, ref } from 'vue';
import { state, boot, subscribeQueue, uploadFiles } from './store.js';
import AppSidebar from './components/AppSidebar.vue';
import AppTopBar from './components/AppTopBar.vue';
import HeroSection from './components/HeroSection.vue';
import EditorPanel from './components/EditorPanel.vue';
import QueuePanel from './components/QueuePanel.vue';
import SettingsDialog from './components/SettingsDialog.vue';
import FileBrowserDialog from './components/FileBrowserDialog.vue';
import ToastBar from './components/ToastBar.vue';

let unsubscribeQueue;
let dragDepth = 0;

function carriesFiles(event) {
  return Array.from(event.dataTransfer?.types || []).includes('Files');
}

function onDragEnter(event) {
  if (!carriesFiles(event)) return;
  dragDepth += 1;
  state.dragging = true;
}

function onDragOver(event) {
  if (carriesFiles(event)) event.preventDefault();
}

function onDragLeave(event) {
  if (!carriesFiles(event)) return;
  dragDepth = Math.max(0, dragDepth - 1);
  if (dragDepth === 0) state.dragging = false;
}

function onDrop(event) {
  if (!carriesFiles(event)) return;
  event.preventDefault();
  dragDepth = 0;
  state.dragging = false;
  uploadFiles(event.dataTransfer?.files);
}

function onKeydown(event) {
  if (event.key !== 'Escape') return;
  state.settingsOpen = false;
  if (state.browser.open) state.browser.open = false;
}

onMounted(async () => {
  window.addEventListener('dragenter', onDragEnter);
  window.addEventListener('dragover', onDragOver);
  window.addEventListener('dragleave', onDragLeave);
  window.addEventListener('drop', onDrop);
  window.addEventListener('keydown', onKeydown);
  await boot();
  unsubscribeQueue = subscribeQueue();
});

onBeforeUnmount(() => {
  window.removeEventListener('dragenter', onDragEnter);
  window.removeEventListener('dragover', onDragOver);
  window.removeEventListener('dragleave', onDragLeave);
  window.removeEventListener('drop', onDrop);
  window.removeEventListener('keydown', onKeydown);
  unsubscribeQueue?.();
});
</script>

<template>
  <div class="app-shell">
    <AppSidebar />
    <div class="work-area">
      <AppTopBar />
      <main class="content">
        <HeroSection />
        <section class="editor-grid">
          <EditorPanel />
          <QueuePanel />
        </section>
      </main>
    </div>
    <SettingsDialog />
    <FileBrowserDialog />
    <ToastBar />
    <div v-if="state.dragging" class="drop-overlay"><div>松手即添加文件（会先复制到本机临时目录）</div></div>
  </div>
</template>
