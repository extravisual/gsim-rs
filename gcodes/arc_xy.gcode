; Creates arcs on XY Plane

G54;
G90;
X0. Y0. Z0.; Move to center

G91;
G1 X100. F50.;

G90; Absolute mode
G2 X-100. Z-50. I-100.; 180 Deg clockwise arc with ramp

G3 X0. Y100. I100.; 270 Deg anticlockwise arc without ramp

G3 X-100. Y0. Z-100. J-100.; 90 Deg anticlockwise arc with ramp

G53 Z0.;
G53 X0. Y0.;
M30;
