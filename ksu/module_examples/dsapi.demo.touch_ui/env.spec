# key|default|type|label|description
TOUCH_DEMO_LAYER_NAME|DirectScreenAPI-TouchGPU|text|Layer Name|窗口层名称
TOUCH_DEMO_LAYER_Z|2100000000|number|Layer Z|窗口层 z（强制高层，避免被前台应用遮挡）
TOUCH_DEMO_RUN_SECONDS|0|number|Run Seconds|0=持续运行

# 统一模式（GPU 渲染 + 触摸/拖拽/关闭/输入 + blur）
TOUCH_DEMO_FILTER_CHAIN||text|Filter Chain|留空=FILTER_CLEAR；可配置链式模糊
TOUCH_DEMO_BLUR_RADIUS|0|number|Blur Radius|presenter 全局模糊半径
TOUCH_DEMO_BLUR_SIGMA|0|number|Blur Sigma|模糊 sigma
TOUCH_DEMO_PRESENTER_FRAME_RATE|current|text|Presenter Frame Rate|auto/max/current/具体帧率
TOUCH_DEMO_FPS|60|number|Demo FPS|触摸窗口渲染帧率
TOUCH_DEMO_NO_TOUCH_ROUTER|0|bool|No Touch Router|1=仅渲染，不接管触控
TOUCH_DEMO_DEVICE||text|Input Device|指定触摸设备路径，如 /dev/input/event7
TOUCH_DEMO_WINDOW_X||number|Window X|初始窗口 X
TOUCH_DEMO_WINDOW_Y||number|Window Y|初始窗口 Y
TOUCH_DEMO_WINDOW_W|620|number|Window Width|初始窗口宽度
TOUCH_DEMO_WINDOW_H|440|number|Window Height|初始窗口高度
TOUCH_DEMO_ROUTE_NAME|demo-window|text|Route Name|触摸路由名称
TOUCH_DEMO_STOP_DAEMON|0|bool|Stop Daemon On Stop|stop 时是否顺带停止核心
