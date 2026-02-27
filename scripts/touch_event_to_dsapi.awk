BEGIN {
  if (max_x <= 0 || max_y <= 0 || width <= 0 || height <= 0) {
    print "touch_bridge_error=invalid_mapping_params" > "/dev/stderr";
    exit 2;
  }
  if (rotation < 0 || rotation > 3) {
    rotation = 0;
  }
  cur_slot = 0;
}

function hex_to_i(hex, v) {
  v = tolower(hex);
  gsub(/^0x/, "", v);
  if (v ~ /^f+$/) {
    return -1;
  }
  return strtonum("0x" v);
}

function clamp01(v) {
  if (v < 0) return 0;
  if (v > 1) return 1;
  return v;
}

function map_point(raw_x, raw_y,   ux, uy, lx, ly) {
  ux = clamp01(raw_x / max_x);
  uy = clamp01(raw_y / max_y);

  if (rotation == 1) {
    lx = uy;
    ly = 1 - ux;
  } else if (rotation == 2) {
    lx = 1 - ux;
    ly = 1 - uy;
  } else if (rotation == 3) {
    lx = 1 - uy;
    ly = ux;
  } else {
    lx = ux;
    ly = uy;
  }

  mapped_x = lx * (width - 1);
  mapped_y = ly * (height - 1);
}

function emit_cmd(kind, pid, raw_x, raw_y) {
  if (pid < 0) return;
  map_point(raw_x, raw_y);

  if (kind == "down") {
    printf("TOUCH_DOWN %d %.3f %.3f\n", pid, mapped_x, mapped_y);
  } else if (kind == "move") {
    printf("TOUCH_MOVE %d %.3f %.3f\n", pid, mapped_x, mapped_y);
  } else if (kind == "up") {
    printf("TOUCH_UP %d %.3f %.3f\n", pid, mapped_x, mapped_y);
  }
}

function set_pending(slot, kind, pid, raw_x, raw_y) {
  pending_kind[slot] = kind;
  pending_pid[slot] = pid;
  pending_x[slot] = raw_x;
  pending_y[slot] = raw_y;
}

function flush_pending(   slot) {
  for (slot in pending_kind) {
    emit_cmd(pending_kind[slot], pending_pid[slot], pending_x[slot], pending_y[slot]);
  }
  delete pending_kind;
  delete pending_pid;
  delete pending_x;
  delete pending_y;
  fflush();
}

$0 ~ /ABS_MT_SLOT/ {
  cur_slot = hex_to_i($NF);
  next;
}

$0 ~ /ABS_MT_TRACKING_ID/ {
  tid = hex_to_i($NF);
  if (tid < 0) {
    if (slot_active[cur_slot]) {
      set_pending(cur_slot, "up", slot_pid[cur_slot], slot_x[cur_slot], slot_y[cur_slot]);
    }
    slot_active[cur_slot] = 0;
    slot_pid[cur_slot] = -1;
    next;
  }

  slot_active[cur_slot] = 1;
  slot_pid[cur_slot] = tid;
  set_pending(cur_slot, "down", tid, slot_x[cur_slot], slot_y[cur_slot]);
  next;
}

$0 ~ /ABS_MT_POSITION_X/ {
  slot_x[cur_slot] = hex_to_i($NF);
  if (slot_active[cur_slot] && !(cur_slot in pending_kind)) {
    set_pending(cur_slot, "move", slot_pid[cur_slot], slot_x[cur_slot], slot_y[cur_slot]);
  } else if ((cur_slot in pending_kind)) {
    pending_x[cur_slot] = slot_x[cur_slot];
  }
  next;
}

$0 ~ /ABS_MT_POSITION_Y/ {
  slot_y[cur_slot] = hex_to_i($NF);
  if (slot_active[cur_slot] && !(cur_slot in pending_kind)) {
    set_pending(cur_slot, "move", slot_pid[cur_slot], slot_x[cur_slot], slot_y[cur_slot]);
  } else if ((cur_slot in pending_kind)) {
    pending_y[cur_slot] = slot_y[cur_slot];
  }
  next;
}

$0 ~ /SYN_REPORT/ {
  flush_pending();
  next;
}
