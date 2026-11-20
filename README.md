# ⏱️ Stream Timer (`stream_timer_rs`)

**Stream Timer** là bộ đồng hồ bấm giờ (`00:00:00` → vô hạn giờ) siêu nhẹ, hiệu năng cao dành riêng cho **Livestreamer, OBS Studio, vMix, PRISM Live, TikTok Live Studio và bộ ghi hình màn hình trên Windows**, được viết 100% bằng **Rust + Win32 API thuần + Direct2D/DirectWrite** (không phụ thuộc framework giao diện nặng, không ngốn RAM/GPU).

Điểm đặc biệt nhất của **Stream Timer** là khả năng **thu gọn/ẩn cửa sổ xuống nền hoặc xuống Khay hệ thống (System Tray) mà hình ảnh đồng hồ trong OBS vẫn tiếp tục nhảy số mượt mà theo thời gian thực**, không bao giờ bị đứng hình (freeze) như các phần mềm đồng hồ thông thường khi bấm Minimize.

---

## 🌟 Tính năng nổi bật (Key Features)

### 1. Chống đứng hình trên OBS khi thu nhỏ (5 Chế độ Minimize Stream-Safe)
Khi bạn bấm nút thu nhỏ (`_`) trên một cửa sổ Windows thông thường, hệ điều hành sẽ đặt cờ `WS_ICONIC` và ngừng vẽ bề mặt DWM của cửa sổ đó, khiến nguồn quay **Window Capture** trên OBS bị đóng băng ở giây cuối cùng. **Stream Timer** giải quyết triệt để bằng **5 chế độ Minimize thông minh**:

| Chế độ (Minimize Mode) | Tham số CLI (`--mode`) | Hiển thị trên Taskbar / Tray | Tương thích OBS / Phần mềm Stream | Cơ chế hoạt động |
| :--- | :--- | :--- | :--- | :--- |
| **1. System Tray (WGC)** *(Mặc định từ v0.2.0)* | `tray_wgc` | Ẩn khỏi Taskbar (qua COM `ITaskbarList`), thu vào **System Tray** | **OBS / TikTok Live Studio Window Capture (WGC)** + **Browser Source** | Ẩn nút Taskbar bằng giao diện COM `ITaskbarList::DeleteTab` mà không bật `WS_EX_TOOLWINDOW`, giúp **OBS / TikTok Live Studio vẫn nhìn thấy và chọn được cửa sổ ngay cả khi đã thu vào khay hệ thống**. Chống tự bật lại khi chuyển focus. |
| **2. Background (WGC)** | `background_wgc` | Giữ trên Taskbar | **OBS Window Capture** (`Windows 10 1903+ / WGC`) | Đưa cửa sổ xuống lớp nền dưới cùng (`HWND_BOTTOM`) thay vì đóng băng `WS_ICONIC`, giữ bề mặt DirectWrite luôn cập nhật mỗi giây. |
| **3. System Tray + Corner Peek 2px** | `tray_corner_peek` | Ẩn khỏi Taskbar, thu vào **System Tray** | Mọi trình quay màn hình (**OBS BitBlt cũ**, Discord, Zoom...) | Neo 2px của cửa sổ ở góc dưới cùng bên phải màn hình và thu vào System Tray để cả trình quay `BitBlt` đời cũ lẫn `WGC` đều bắt được hình. |
| **4. Corner Peek 2px** | `corner_peek` | Giữ trên Taskbar | **OBS BitBlt** & **WGC** | Neo 2px ở góc dưới màn hình nhưng vẫn giữ biểu tượng trên thanh Taskbar để click mở lại nhanh. |
| **5. Native Minimize** | `native` | Thu nhỏ chuẩn Windows | Chỉ dùng cho **OBS Browser Source** | Thu nhỏ cửa sổ truyền thống (`SW_MINIMIZE`) khi bạn chỉ lấy hình qua đường dẫn Web Localhost. |

> 💡 **Tùy chọn ẩn thêm khỏi `Alt + Tab` (v0.2.0):** Trong menu chuột phải có thêm mục **`Ẩn khỏi Alt+Tab khi ở khay (chọn nguồn OBS trước)`**. Mặc định mục này tắt để bạn có thể chọn nguồn quay Window Capture trong OBS/TikTok bất cứ lúc nào (kể cả khi đồng hồ đang ở dưới khay). Sau khi đã thêm nguồn vào OBS xong, bạn có thể tích bật mục này để ẩn hoàn toàn đồng hồ khỏi bảng `Alt + Tab`.

---

### 2. Tích hợp sẵn Máy chủ Web nội bộ cho OBS Browser Source (`127.0.0.1`)
Ngay khi khởi động, ứng dụng tự động bật một HTTP Server siêu nhẹ chạy ngầm (mặc định tại cổng `18080`, tự động chuyển cổng dự phòng nếu cổng bận):
- **Nền đen thuần (`#000000`):** `http://127.0.0.1:18080/`
- **Nền trong suốt hoàn toàn (`Transparent` — đè trực tiếp lên cảnh game/camera không cần Chroma Key):** `http://127.0.0.1:18080/?bg=transparent`
- Tự động đồng bộ trạng thái **Pause / Resume / Reset / Khôi phục phiên** với cửa sổ chính ở độ trễ `< 250ms` và nội suy khung hình `requestAnimationFrame` mượt tuyệt đối.

---

### 3. Đồ họa Direct2D + DirectWrite sắc nét & Tự động co giãn vô hạn giờ
- Sử dụng **DirectWrite Grayscale Anti-Aliasing** kết hợp bộ đệm đôi **DIB Double-Buffering (`BitBlt`)** giúp chữ số trắng trên nền đen `#000000` sắc nét tuyệt đối ở mọi độ phân giải (**Per-Monitor DPI Aware v2**).
- **Thuật toán Binary-Search Font Auto-Scaling:** Tự động tính toán cỡ chữ lớn nhất vừa khít khung cửa sổ khi bạn kéo giãn kích thước bất kỳ (từ kích thước nhỏ `180x68` lên tới toàn màn hình 4K).
- **Hỗ trợ đếm vô hạn giờ (`00:00:00` → `99:59:59` → `100:00:00` → `999999:59:59`):** Khi số chữ số giờ tăng lên, phông chữ trên cả cửa sổ Win32 lẫn trang Browser Source đều tự động thu phóng mượt mà, tuyệt đối không bao giờ bị tràn hay cắt chữ.

---

### 4. Tự động lưu & Khôi phục phiên trước sau khi tắt máy / mất điện (Ping-Pong Session Persistence)
- Ứng dụng tự động lưu thời gian thực mỗi giây vào 2 tệp luân phiên (`session_0.txt` và `session_1.txt` tại `%LOCALAPPDATA%\StreamTimer`) theo cơ chế **Ping-Pong 2-Slot Atomic Write**.
- Khi bạn mở lại ứng dụng cho buổi stream tiếp theo:
  - Lần chạy mới **không bao giờ ghi đè làm mất kết quả của phiên liền trước**.
  - Thanh tiêu đề sẽ nhắc nhẹ **`Ctrl+S: Resume <thời_gian_cũ>`** trong 3.5 giây đầu.
  - Chỉ cần bấm **`Ctrl + S`** (hoặc chọn trong menu chuột phải), đồng hồ sẽ ngay lập tức nối tiếp đúng số giờ:phút:giây của phiên trước!

---

## ⌨️ Bảng Phím Tắt & Thao Tác Điều Khiển (Keyboard Shortcuts & Controls)

### 1. Phím tắt khi đang chọn cửa sổ Stream Timer (In-App Hotkeys)

| Phím tắt | Chức năng | Mô tả chi tiết |
| :--- | :--- | :--- |
| **`Space`** *(Phím cách)* | **Tạm dừng / Tiếp tục (Pause / Resume)** | Tạm dừng đồng hồ (tiêu đề hiện `Stream Timer [Paused]`) hoặc chạy tiếp. |
| **`R`** | **Đặt lại về `00:00:00` (Reset)** | Reset bộ đếm về `00:00:00` và tiếp tục đếm ngay lập tức. |
| **`Ctrl + S`** | **Khôi phục phiên trước (Resume Previous Session)** | Nạp lại thời gian đã lưu của lần mở ứng dụng liền trước đó (chỉ nhận 1 lần để tránh bấm nhầm). |
| **`H`** hoặc **`Ctrl + M`** | **Thu gọn vào Khay hệ thống (Minimize to System Tray)** | Ẩn hoàn toàn cửa sổ khỏi màn hình, Taskbar và `Alt + Tab` xuống góc đồng hồ hệ thống (OBS vẫn quay bình thường). |
| **`M`** *(hoặc nút `_` trên cửa sổ)* | **Minimize Stream-Safe** | Thu gọn cửa sổ theo chế độ Minimize Mode đang chọn mà không làm đứng hình OBS. |
| **`T`** | **Luôn nổi trên cùng (Always on Top)** | Ghim hoặc bỏ ghim cửa sổ Stream Timer nổi lên trên tất cả các ứng dụng khác. |
| **`C`** | **Copy URL Browser Source** | Sao chép nhanh địa chỉ `http://127.0.0.1:18080/` vào Clipboard để dán vào OBS. |
| **`Esc`** hoặc **`Q`** | **Thoát ứng dụng (Quit)** | Lưu trạng thái giây cuối cùng, gỡ biểu tượng khay hệ thống và đóng ứng dụng an toàn. |

### 2. Phím tắt Toàn cục (Global Hotkey — Hoạt động ở mọi nơi, kể cả khi đang trong Game)

| Phím tắt toàn cục | Chức năng |
| :--- | :--- |
| **`Ctrl + Shift + H`** | **Hiện / Ẩn nhanh Stream Timer từ Khay hệ thống (System Tray)** ở bất kỳ đâu mà không cần `Alt + Tab` ra ngoài màn hình Desktop. |

### 3. Thao tác Chuột & Khay Hệ Thống (Mouse & System Tray)
- **Nhấp chuột phải vào bất kỳ đâu trong cửa sổ đồng hồ:** Mở **Menu ngữ cảnh Dark Mode** đầy đủ mọi chức năng:
  - *Tiếp tục phiên trước (`Ctrl+S`)* (nếu có phiên cũ)
  - *Pause / Resume (`Space`)*
  - *Reset 00:00:00 (`R`)*
  - *Thu vào khay hệ thống (`H / Ctrl+Shift+H`)*
  - *Minimize Stream-Safe (`M`)*
  - *Minimize Mode* → chuyển đổi trực tiếp giữa 5 chế độ tương thích OBS
  - *Always on Top (`T`)*
  - *Copy Browser Source URL* & *Copy Transparent Browser URL (`?bg=transparent`)*
  - *Quit (`Esc`)*
- **Ở biểu tượng dưới Khay hệ thống (System Tray góc phải thanh Taskbar):**
  - **Nhấp chuột trái (Left-Click):** Bật / Ẩn nhanh cửa sổ Stream Timer.
  - **Nhấp chuột phải (Right-Click):** Mở menu điều khiển nhanh ngay từ khay hệ thống mà không cần mở cửa sổ lên màn hình.

---

## 🎥 Hướng dẫn thiết lập trong OBS Studio (OBS Setup Guide)

### Cách A: Dùng OBS Browser Source (Khuyên dùng — Gọn nhẹ & Hỗ trợ nền trong suốt)
1. Mở `stream_timer.exe`.
2. Nhấp chuột phải vào cửa sổ đồng hồ → chọn **`Copy Transparent Browser URL`** (hoặc **`Copy Browser Source URL`** nếu muốn giữ nền đen).
3. Trong **OBS Studio**, tại khung **Sources** → bấm dấu **`+`** → chọn **`Browser`**:
   - **URL:** Dán (`Ctrl + V`) đường dẫn vừa copy (ví dụ: `http://127.0.0.1:18080/?bg=transparent`).
   - **Width:** `440` (hoặc `800` nếu muốn chữ siêu nét).
   - **Height:** `140` (hoặc `250`).
4. Bấm **OK**. Bạn có thể bấm phím **`H`** trên cửa sổ Stream Timer để giấu nó xuống System Tray cho gọn màn hình.

### Cách B: Dùng OBS Window Capture (Quay trực tiếp cửa sổ ứng dụng)
1. Mở `stream_timer.exe`.
2. Trong **OBS Studio**, tại khung **Sources** → bấm dấu **`+`** → chọn **`Window Capture`**:
   - **Window:** Chọn `[stream_timer.exe]: Stream Timer`.
   - **Capture Method:** Chọn **`Windows 10 (1903 and up)`** (WGC).
3. Khi cần giấu cửa sổ đồng hồ đi để làm việc khác:
   - Chỉ cần bấm nút thu nhỏ **`_`** trên góc cửa sổ (hoặc bấm phím **`M`** để đưa xuống nền, hoặc phím **`H`** để giấu vào System Tray).
   - Nhờ cơ chế **Stealth Minimize**, đồng hồ trên OBS vẫn tiếp tục đếm mượt mà không bị dừng hình!

---

## ⚙️ Tham số dòng lệnh nâng cao (CLI Options)

Bạn có thể truyền thêm các cờ tùy chỉnh khi tạo Shortcut hoặc gọi qua Terminal/Script:

```powershell
stream_timer.exe [OPTIONS]
```

| Tham số | Mặc định | Ý nghĩa |
| :--- | :--- | :--- |
| `--mode <MODE>` | `background_wgc` | Chọn sẵn chế độ Minimize khi khởi động: `background_wgc`, `tray_wgc`, `tray_corner_peek`, `corner_peek`, hoặc `native`. |
| `--port <PORT>` | `18080` | Chỉ định cổng HTTP cho OBS Browser Source (tự động chọn cổng trống nếu cổng này đang bận). |
| `--width <PX>` | `440` | Chiều rộng vùng hiển thị ban đầu của cửa sổ (tối thiểu `180`). |
| `--height <PX>` | `140` | Chiều cao vùng hiển thị ban đầu của cửa sổ (tối thiểu `68`). |
| `--session-dir <PATH>` | `%LOCALAPPDATA%\StreamTimer` | Thư mục tùy chỉnh để lưu 2 tệp khôi phục phiên (`session_0.txt`, `session_1.txt`). |
| `--allow-multi` | *(Tắt)* | Cho phép mở nhiều cửa sổ đồng hồ độc lập cùng lúc (mặc định ứng dụng khóa Single-Instance và đánh thức cửa sổ đang chạy). |

---

## 📦 Tải về & Tự biên dịch (Download & Build)

### 1. Tải bản thực thi đóng gói sẵn (Portable `.exe`)
- Truy cập mục **[Releases](../../releases/latest)** ở cột bên phải của trang GitHub này.
- Tải **`StreamTimer-windows-x64.zip`** hoặc **`stream_timer.exe`** (File đơn duy nhất < 500 KB, chạy ngay trên mọi máy Windows 10/11 không cần cài đặt thêm bất kỳ thư viện nào).

### 2. Tự biên dịch từ mã nguồn Rust
Yêu cầu: **Rust Toolchain** (`rustc 1.85+`, Edition 2024) trên Windows 10/11.

```powershell
git clone https://github.com/kumalalalala/stream_timer_rs.git
cd stream_timer_rs

# Biên dịch bản Release tối ưu (Fat LTO + Strip)
cargo build --release

# Chạy bộ kiểm thử tự động (Timer Engine, HTTP Browser Server, DirectWrite Pixel AA, Ping-Pong Session)
cargo test
```
