package com.mozilla.servo;

import android.content.Context;
import android.opengl.GLSurfaceView;
import android.util.AttributeSet;
import android.util.Log;

public class ServoGLView extends GLSurfaceView {

    private static final String LOGTAG = "ServoGLView";

    private final ServoGLRenderer mRenderer;
    private final NativeServo mServo;

    public ServoGLView(Context context, AttributeSet attrs) {
        super(context, attrs);
        setEGLContextClientVersion(3);
        setEGLConfigChooser(8, 8, 8, 8, 24, 0);
        mRenderer = new ServoGLRenderer();
        setRenderer(mRenderer);
        mServo = new NativeServo();
        Log.d(LOGTAG, "Servo Version: " + mServo.version());
        //setRenderMode(GLSurfaceView.RENDERMODE_WHEN_DIRTY);
    }

}
