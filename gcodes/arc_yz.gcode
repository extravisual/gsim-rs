; Creates arcs on XZ Plane

G19;
G54;
G90;
X0. Y0. Z0.; Move to center

G91;
G1 Y100. F50.;

G90; Absolute mode
G2 Y-100. X-50. J-100.; 180 Deg clockwise arc with ramp

G3 Y0. Z100. J100.; 270 Deg anticlockwise arc without ramp

G3 Y-100. Z0. X-100. K-100.; 90 Deg anticlockwise arc with ramp

G53 Z0.;
G53 X0. Y0.;
M30;
