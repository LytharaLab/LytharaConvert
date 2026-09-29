<script setup>
import { computed } from 'vue';
import { state, config, cancelJob, clearFinished, openOutput, displayName, extOf, size } from '../store.js';
import AppIcon from './AppIcon.vue';

const doneCount = computed(() => state.jobs.filter(job => job.status === 'done').length);
const visibleJobs = computed(() => state.jobs.slice().reverse());

const STATUS = {
  pending: '等待中',
  done: '已完成',
  error: '失败',
  cancelled: '已取消'
};

function statusText(job) {
  if (job.status === 'running') return `${job.progress}%`;
  return STATUS[job.status] || job.status;
}

function label(job) {
  if (job.mode === 'gif') return `GIF · ${job.sources?.length || 1} 帧`;
  return (job.format || extOf(job.source)).toUpperCase();
}

function meta(job) {
  const parts = [label(job), size(job.inputBytes)];
  if (job.status === 'done') parts.push(`→ ${size(job.bytes)}`);
  if (job.frames) parts.push(`${job.frames} 帧`);
  return parts.join(' · ');
}
</script>

<template>
  <aside class="queue-column">
    <section class="panel queue-panel">
      <div class="queue-top">
        <div><span class="eyebrow side-eyebrow">LIVE ACTIVITY</span><h2>处理队列</h2></div>
        <span class="queue-count">{{ state.jobs.length }}</span>
      </div>
      <div class="queue-stat">
        <div class="stat-icon"><AppIcon name="layers" /></div>
        <div><b>{{ doneCount }}</b><span>已完成任务</span></div>
        <div class="stat-orbit"></div>
      </div>
      <div class="queue-list-head">
        <span>当前任务</span>
        <button class="text-button subtle" @click="clearFinished">清除已完成</button>
      </div>
      <div class="queue-list">
        <div v-if="!state.jobs.length" class="queue-empty">
          <span><AppIcon name="layers" /></span>
          <b>队列还是空的</b>
          <span>添加文件，第一次转换即刻开始</span>
        </div>
        <div v-for="job in visibleJobs" :key="job.id" class="queue-task">
          <div class="task-top">
            <div class="task-type">{{ label(job).slice(0, 4) }}</div>
            <div class="task-meta">
              <b :title="job.source">{{ displayName(job.source) }}</b>
              <small>{{ meta(job) }}</small>
            </div>
            <span class="task-status" :class="job.status">{{ statusText(job) }}</span>
          </div>
          <div v-if="job.status === 'running'" class="task-progress">
            <span :style="{ width: `${job.progress}%` }"></span>
          </div>
          <p v-if="job.error" class="task-error" :title="job.error">{{ job.error }}</p>
          <div v-if="['running', 'pending', 'done'].includes(job.status)" class="task-actions">
            <button @click="job.status === 'done' ? openOutput(job.output) : cancelJob(job.id)">
              {{ job.status === 'done' ? '查看文件 ↗' : '取消任务' }}
            </button>
          </div>
        </div>
      </div>
      <button class="open-folder" @click="openOutput()"><AppIcon name="folder" /> 打开输出文件夹 <span>↗</span></button>
    </section>
    <div class="tip-panel">
      <span class="tip-icon">✦</span>
      <div><strong>一个小提示</strong><p>{{ config.tip }}</p></div>
    </div>
  </aside>
</template>
